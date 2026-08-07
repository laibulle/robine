import SwiftUI
import RobineClient

@MainActor
public final class HomeViewModel: ObservableObject {
    @Published public private(set) var devices: [RobineDevice] = []
    @Published public private(set) var areas: [RobineArea] = []
    @Published public private(set) var states: [UUID: [String: RobineStateProperty]] = [:]
    @Published public private(set) var pending: [UUID: PendingCommand] = [:]
    @Published public private(set) var automations: [RobineAutomation] = []
    @Published public private(set) var recentEvents: [RobineEvent] = []
    @Published public private(set) var message = "Robine veille tranquillement sur la maison."
    private let client: RobineClient
    private let cache: any RobinePresentationStore
    private let server: RobineServer
    private var realtimeTask: Task<Void, Never>?
    private var cursor: UInt64 = 0

    public init(client: RobineClient, cache: any RobinePresentationStore = UserDefaultsPresentationStore()) {
        self.client = client
        self.cache = cache
        server = client.server
        do {
            if let snapshot = try cache.load(for: server) {
                cursor = snapshot.cursor
                devices = snapshot.devices
                areas = snapshot.areas
                states = Dictionary(grouping: snapshot.states, by: \.entityId)
                    .mapValues { Dictionary(uniqueKeysWithValues: $0.map { ($0.key, $0) }) }
                message = "Dernier état connu — resynchronisation en cours."
            }
        } catch {
            message = "Le cache local sera reconstruit à la prochaine connexion."
        }
    }

    public func refresh() async {
        do {
            try await synchronize()
            try persistSnapshot()
            message = "La maison est à jour."
        }
        catch { message = error.localizedDescription }
    }

    public func setSwitch(_ entity: RobineEntity, on: Bool) async {
        do {
            _ = try await client.requestCommand(entityID: entity.id, key: "switch", value: on)
            pending[entity.id] = PendingCommand(key: "switch", expected: .bool(on))
            message = on ? "Demande d’allumage envoyée à \(entity.name), en attente de confirmation." : "Demande d’extinction envoyée à \(entity.name), en attente de confirmation."
        } catch { message = error.localizedDescription }
    }

    public func setBrightness(_ entity: RobineEntity, percentage: Double) async {
        do {
            _ = try await client.requestCommand(
                entityID: entity.id,
                key: "light.brightness",
                value: percentage
            )
            pending[entity.id] = PendingCommand(key: "light.brightness", expected: .percentage(percentage))
            message = "Demande de luminosité envoyée à \(entity.name), en attente de confirmation."
        } catch { message = error.localizedDescription }
    }

    public func setColorTemperature(_ entity: RobineEntity, mirek: UInt16) async {
        do {
            _ = try await client.requestCommand(
                entityID: entity.id,
                key: "light.color_temperature",
                value: String(mirek)
            )
            pending[entity.id] = PendingCommand(key: "light.color_temperature", expected: .text(String(mirek)))
            message = "Demande de teinte envoyée à \(entity.name), en attente de confirmation."
        } catch { message = error.localizedDescription }
    }

    public func entities(in areaID: UUID?) -> [RobineEntity] {
        devices
            .flatMap(\.entities)
            .filter { $0.areaId == areaID }
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    public var needsAttention: Bool {
        devices.contains { $0.status == "unavailable" }
    }

    public func state(of entity: RobineEntity, key: String) -> RobineStateProperty? {
        states[entity.id]?[key]
    }

    private func refreshStates(for entities: [RobineEntity]) async throws {
        var fetched: [UUID: [String: RobineStateProperty]] = [:]
        try await withThrowingTaskGroup(of: RobineEntityDetail.self) { group in
            for entity in entities { group.addTask { try await self.client.entityDetail(entity.id) } }
            for try await detail in group {
                let byKey = Dictionary(uniqueKeysWithValues: detail.state.map { ($0.key, $0) })
                fetched[detail.entity.id] = byKey
            }
        }
        states = fetched
        pending = pending.filter { entityID, command in
            guard let actual = fetched[entityID]?[command.key]?.value else { return true }
            return actual != command.expected
        }
    }

    private func synchronize() async throws {
        async let fetchedDevices = client.devices()
        async let fetchedAreas = client.areas()
        async let fetchedAutomations = try? client.automations()
        async let fetchedEvents = try? client.recentEvents()
        devices = try await fetchedDevices
        areas = try await fetchedAreas
        try await refreshStates(for: devices.flatMap(\.entities))
        automations = await fetchedAutomations ?? []
        recentEvents = await fetchedEvents?.events ?? []
    }

    private func persistSnapshot() throws {
        let snapshot = RobinePresentationSnapshot(
            cursor: cursor,
            devices: devices,
            areas: areas,
            states: states.values.flatMap { $0.values }
        )
        try cache.save(snapshot, for: server)
    }

    public func startRealtime() {
        realtimeTask?.cancel()
        realtimeTask = Task { [weak self] in
            guard let self else { return }
            do {
                let stream = try await RobineEventStream(client: self.client, after: self.cursor)
                try await stream.connect()
                while !Task.isCancelled {
                    switch try await stream.next() {
                    case .ready:
                        // Le curseur persistant reste celui du dernier événement
                        // dont la projection a réellement été appliquée.
                        break
                    case let .event(id, topic, _, _):
                        if ["state", "device", "area", "adapter"].contains(topic) {
                            try await self.synchronize()
                        }
                        self.cursor = id
                        try self.persistSnapshot()
                        try await stream.acknowledge(id)
                    case .resyncRequired:
                        self.cursor = 0
                        try await self.synchronize()
                        try self.persistSnapshot()
                    }
                }
                await stream.close()
            } catch where !Task.isCancelled {
                self.message = "La maison sera resynchronisée au prochain retour de l’app."
            } catch { }
        }
    }

    deinit { realtimeTask?.cancel() }
}

public struct PendingCommand: Equatable {
    let key: String
    let expected: RobineStateValue
}

public struct HomeView: View {
    @StateObject private var model: HomeViewModel
    public init(client: RobineClient) { _model = StateObject(wrappedValue: HomeViewModel(client: client)) }
    public var body: some View {
        NavigationStack {
            List {
                Section {
                    HStack(spacing: 12) {
                        Image(systemName: model.needsAttention ? "pawprint.fill" : "pawprint")
                            .font(.title2)
                            .foregroundStyle(model.needsAttention ? .orange : .mint)
                            .accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(model.needsAttention ? "Robine est attentive" : "Robine veille tranquillement")
                                .font(.headline)
                            Text(model.needsAttention ? "Un appareil mérite un coup d’œil." : "La maison est douce.")
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                        }
                    }
                    .accessibilityElement(children: .combine)
                }
                ForEach(model.areas) { area in
                    let entities = model.entities(in: area.id)
                    if !entities.isEmpty {
                        Section(area.name) {
                            ForEach(entities) { entity in
                                LightRow(entity: entity, model: model)
                            }
                        }
                    }
                }
                let unassigned = model.entities(in: nil)
                if !unassigned.isEmpty {
                    Section("Sans pièce") {
                        ForEach(unassigned) { entity in
                            LightRow(entity: entity, model: model)
                        }
                    }
                }
                if !model.automations.isEmpty {
                    Section("Habitudes") {
                        ForEach(model.automations.prefix(3)) { automation in
                            HStack {
                                Image(systemName: automation.enabled ? "sparkles" : "pause.circle")
                                    .foregroundStyle(automation.enabled ? .mint : .secondary)
                                    .accessibilityHidden(true)
                                VStack(alignment: .leading) {
                                    Text(automation.name)
                                    Text(automation.enabled ? "Active" : "En pause")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
                            .accessibilityLabel("Habitude \(automation.name), \(automation.enabled ? "active" : "en pause")")
                        }
                    }
                }
                if !model.recentEvents.isEmpty {
                    Section("Activité récente") {
                        ForEach(model.recentEvents.suffix(5).reversed()) { event in
                            VStack(alignment: .leading) {
                                Text(event.eventType)
                                Text(event.occurredAt.formatted(date: .abbreviated, time: .shortened))
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            .accessibilityLabel("\(event.eventType), \(event.occurredAt.formatted(date: .abbreviated, time: .shortened))")
                        }
                    }
                }
            }
            .navigationTitle("Robine")
            .safeAreaInset(edge: .bottom) {
                Text(model.message)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding()
                    .background(.thinMaterial)
            }
            .task { await model.refresh(); model.startRealtime() }
            .refreshable { await model.refresh() }
        }
    }
}

private struct LightRow: View {
    let entity: RobineEntity
    @ObservedObject var model: HomeViewModel

    var body: some View {
        HStack {
            VStack(alignment: .leading) {
                Text(entity.name)
                Text(statusText).font(.caption).foregroundStyle(.secondary)
            }
            if model.pending[entity.id] != nil {
                ProgressView().controlSize(.small).accessibilityLabel("Commande en attente de confirmation")
            }
            Spacer()
            if entity.capabilities.contains(where: { $0.key == "switch" }) {
                Button("Allumer") { Task { await model.setSwitch(entity, on: true) } }
                Button("Éteindre") { Task { await model.setSwitch(entity, on: false) } }
            }
            if entity.capabilities.contains(where: { $0.key == "light.brightness" }) {
                Menu("Lumière") {
                    Button("Douce · 30 %") { Task { await model.setBrightness(entity, percentage: 30) } }
                    Button("Pleine · 100 %") { Task { await model.setBrightness(entity, percentage: 100) } }
                }
            }
            if entity.capabilities.contains(where: { $0.key == "light.color_temperature" }) {
                Menu("Teinte") {
                    Button("Chaleureuse") { Task { await model.setColorTemperature(entity, mirek: 370) } }
                    Button("Neutre") { Task { await model.setColorTemperature(entity, mirek: 300) } }
                    Button("Fraîche") { Task { await model.setColorTemperature(entity, mirek: 220) } }
                }
            }
        }
    }

    private var statusText: String {
        if let state = model.state(of: entity, key: "switch")?.value {
            if case let .bool(on) = state { return on ? "Allumée" : "Éteinte" }
        }
        return entity.kind == "light" ? "État en attente" : entity.kind
    }
}
