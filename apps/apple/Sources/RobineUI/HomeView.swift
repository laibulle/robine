import SwiftUI
import RobineClient

@MainActor
public final class HomeViewModel: ObservableObject {
    @Published public private(set) var devices: [RobineDevice] = []
    @Published public private(set) var areas: [RobineArea] = []
    @Published public private(set) var states: [UUID: [String: RobineStateProperty]] = [:]
    @Published public private(set) var pending: [UUID: PendingCommand] = [:]
    @Published public private(set) var automations: [RobineAutomation] = []
    @Published public private(set) var pendingAutomationIDs: Set<UUID> = []
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
                automations = snapshot.automations
                recentEvents = snapshot.recentEvents
                message = "Dernier état connu — resynchronisation en cours."
            }
        } catch {
            message = "Le cache local sera reconstruit à la prochaine connexion."
        }
    }

    var clientForAdministration: RobineClient { client }

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

    public func setAutomation(_ automation: RobineAutomation, enabled: Bool) async {
        guard pendingAutomationIDs.insert(automation.id).inserted else { return }
        defer { pendingAutomationIDs.remove(automation.id) }
        do {
            let updated = try await client.updateAutomation(automation, enabled: enabled)
            if let index = automations.firstIndex(where: { $0.id == updated.id }) {
                automations[index] = updated
            }
            try persistSnapshot()
            message = enabled ? "L’habitude \(updated.name) est active." : "L’habitude \(updated.name) est en pause."
        } catch {
            message = error.localizedDescription
        }
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
            states: states.values.flatMap { $0.values },
            automations: automations,
            recentEvents: recentEvents
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
                    case let .event(id, topic, eventType, data):
                        if ["state", "device", "area", "adapter"].contains(topic) {
                            try await self.synchronize()
                        }
                        if topic == "command" {
                            self.applyCommandEvent(type: eventType, data: data)
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

    private func applyCommandEvent(type: String, data: Data) {
        guard let entityID = CommandEventPayload.entityID(from: data) else { return }
        switch type {
        case "command.confirmed":
            pending.removeValue(forKey: entityID)
            message = "La commande a été confirmée par l’appareil."
        case "command.failed":
            pending.removeValue(forKey: entityID)
            message = "La commande n’a pas pu être appliquée."
        case "command.expired":
            pending.removeValue(forKey: entityID)
            message = "La commande n’a pas été confirmée à temps."
        default:
            break
        }
    }
}

public struct PendingCommand: Equatable {
    let key: String
    let expected: RobineStateValue
}

private struct CommandEventPayload: Decodable {
    struct Command: Decodable {
        let entityID: UUID
    }

    let command: Command

    static func entityID(from data: Data) -> UUID? {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try? decoder.decode(CommandEventPayload.self, from: data).command.entityID
    }
}

public struct HomeView: View {
    @StateObject private var model: HomeViewModel
    @State private var showsAdministration = false
    @State private var showsAutomations = false
    #if os(macOS)
    @State private var showsBackup = false
    @State private var showsDiagnostics = false
    #endif
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
                            HStack(spacing: 12) {
                                Image(systemName: automation.enabled ? "sparkles" : "pause.circle")
                                    .foregroundStyle(automation.enabled ? .mint : .secondary)
                                    .accessibilityHidden(true)
                                VStack(alignment: .leading) {
                                    Text(automation.name)
                                    Text(automation.enabled ? "Active" : "En pause")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                                Spacer()
                                if model.pendingAutomationIDs.contains(automation.id) {
                                    ProgressView().controlSize(.small)
                                } else {
                                    Toggle(
                                        "Activer \(automation.name)",
                                        isOn: Binding(
                                            get: { automation.enabled },
                                            set: { enabled in
                                                Task { await model.setAutomation(automation, enabled: enabled) }
                                            }
                                        )
                                    )
                                    .labelsHidden()
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
            .toolbar {
                ToolbarItem {
                    Button("Organiser la maison") { showsAdministration = true }
                }
                ToolbarItem {
                    Button("Habitudes") { showsAutomations = true }
                }
                #if os(macOS)
                ToolbarItem {
                    Button("Sauvegarde") { showsBackup = true }
                }
                ToolbarItem {
                    Button("Diagnostic") { showsDiagnostics = true }
                }
                #endif
            }
            .sheet(isPresented: $showsAdministration, onDismiss: {
                Task { await model.refresh() }
            }) {
                HomeAdministrationView(client: model.clientForAdministration)
            }
            .sheet(isPresented: $showsAutomations, onDismiss: {
                Task { await model.refresh() }
            }) {
                AutomationListView(client: model.clientForAdministration)
            }
            #if os(macOS)
            .sheet(isPresented: $showsBackup) {
                BackupPanel(client: model.clientForAdministration)
            }
            .sheet(isPresented: $showsDiagnostics) {
                DiagnosticsPanel(client: model.clientForAdministration)
            }
            #endif
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

private struct HomeAdministrationView: View {
    @Environment(\.dismiss) private var dismiss
    @StateObject private var model: HomeAdministrationModel
    @State private var newAreaName = ""
    @State private var selectedBridge: HueBridgeCandidate?
    @State private var bridgeCertificate: HueBridgeCertificate?

    init(client: RobineClient) {
        _model = StateObject(wrappedValue: HomeAdministrationModel(client: client))
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Pièces") {
                    HStack {
                        TextField("Nouvelle pièce", text: $newAreaName)
                        Button("Ajouter") {
                            Task {
                                await model.createArea(named: newAreaName)
                                newAreaName = ""
                            }
                        }
                        .disabled(newAreaName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }

                    if model.areas.isEmpty {
                        Text("Créez une pièce pour retrouver facilement les lumières de Robine.")
                            .foregroundStyle(.secondary)
                    } else {
                        ForEach(model.areas) { area in
                            Text(area.name)
                        }
                    }
                }

                Section("Lumières sans pièce") {
                    let unassigned = model.unassignedEntities
                    if unassigned.isEmpty {
                        Text("Toutes les lumières découvertes ont une pièce.")
                            .foregroundStyle(.secondary)
                    } else if model.areas.isEmpty {
                        Text("Créez d’abord une pièce, puis revenez ici pour y placer les lumières.")
                            .foregroundStyle(.secondary)
                    } else {
                        ForEach(unassigned) { entity in
                            Menu {
                                ForEach(model.areas) { area in
                                    Button(area.name) {
                                        Task { await model.assign(entity: entity, to: area) }
                                    }
                                }
                            } label: {
                                HStack {
                                    Text(entity.name)
                                    Spacer()
                                    Text("Choisir une pièce")
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                    }
                }

                Section("Philips Hue") {
                    Text("Robine cherche les bridges sur le réseau local. Appuyez sur le bouton rond du bridge juste avant l’association.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)

                    Button(model.isDiscovering ? "Recherche…" : "Rechercher un bridge") {
                        Task { await model.discoverHue() }
                    }
                    .disabled(model.isDiscovering)

                    ForEach(model.bridges) { bridge in
                        Button {
                            selectedBridge = bridge
                            bridgeCertificate = nil
                            Task { bridgeCertificate = await model.probeCertificate(for: bridge) }
                        } label: {
                            VStack(alignment: .leading) {
                                Text(bridge.name)
                                Text(bridge.host)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }

                    if model.bridges.isEmpty && !model.isDiscovering {
                        Text("Aucun bridge trouvé pour l’instant. Vérifiez qu’il est sur le même réseau local.")
                            .foregroundStyle(.secondary)
                    }
                }

                if let bridge = selectedBridge {
                    Section("Associer \(bridge.name)") {
                        Text("Robine vérifie l’identité du bridge choisi avant de l’épingler. Appuyez maintenant sur son bouton rond, puis confirmez l’association.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                        if model.isProbingCertificate {
                            HStack {
                                ProgressView().controlSize(.small)
                                Text("Vérification de l’identité du bridge…")
                            }
                        } else if let bridgeCertificate {
                            Label("Identité locale prête · \(bridgeCertificate.shortFingerprint)", systemImage: "checkmark.shield")
                                .foregroundStyle(.secondary)
                                .accessibilityLabel("Identité locale du bridge vérifiée, empreinte \(bridgeCertificate.sha256)")
                        } else {
                            Button("Réessayer la vérification") {
                                Task { bridgeCertificate = await model.probeCertificate(for: bridge) }
                            }
                        }
                        Button(model.isPairing ? "Association…" : "J’ai appuyé sur le bouton du bridge") {
                            Task {
                                guard let certificate = bridgeCertificate else { return }
                                await model.pair(bridge: bridge, certificatePEM: certificate.pem)
                                if model.pairingSucceeded { selectedBridge = nil; bridgeCertificate = nil }
                            }
                        }
                        .disabled(model.isPairing || model.isProbingCertificate || bridgeCertificate == nil)
                    }
                }

                if let message = model.message {
                    Section {
                        Text(message)
                            .font(.footnote)
                            .foregroundStyle(model.isError ? .red : .secondary)
                            .accessibilityLabel("Statut : \(message)")
                    }
                }
            }
            .navigationTitle("Organiser Robine")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Terminé", action: { dismiss() })
                }
            }
            .task { await model.load() }
        }
    }
}

@MainActor
private final class HomeAdministrationModel: ObservableObject {
    @Published private(set) var areas: [RobineArea] = []
    @Published private(set) var devices: [RobineDevice] = []
    @Published private(set) var bridges: [HueBridgeCandidate] = []
    @Published private(set) var message: String?
    @Published private(set) var isError = false
    @Published private(set) var isDiscovering = false
    @Published private(set) var isProbingCertificate = false
    @Published private(set) var isPairing = false
    @Published private(set) var pairingSucceeded = false

    private let client: RobineClient

    init(client: RobineClient) { self.client = client }

    var unassignedEntities: [RobineEntity] {
        devices.flatMap(\.entities)
            .filter { $0.areaId == nil }
            .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }

    func load() async {
        do {
            async let fetchedAreas = client.areas()
            async let fetchedDevices = client.devices()
            areas = try await fetchedAreas
            devices = try await fetchedDevices
        } catch {
            report(error)
        }
    }

    func createArea(named name: String) async {
        let normalized = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalized.isEmpty else { return }
        do {
            _ = try await client.createArea(name: normalized)
            message = "La pièce \(normalized) est prête."
            isError = false
            await load()
        } catch {
            report(error)
        }
    }

    func assign(entity: RobineEntity, to area: RobineArea) async {
        do {
            _ = try await client.assign(entityID: entity.id, to: area.id)
            message = "\(entity.name) est maintenant dans \(area.name)."
            isError = false
            await load()
        } catch {
            report(error)
        }
    }

    func discoverHue() async {
        isDiscovering = true
        defer { isDiscovering = false }
        do {
            bridges = try await client.discoverHueBridges()
            message = bridges.isEmpty ? "Aucun bridge Hue découvert." : "Choisissez le bridge à associer."
            isError = false
        } catch {
            report(error)
        }
    }

    func probeCertificate(for bridge: HueBridgeCandidate) async -> HueBridgeCertificate? {
        isProbingCertificate = true
        defer { isProbingCertificate = false }
        do {
            let certificate = try await HueBridgeCertificateProbe.probe(authority: bridge.host)
            message = "L’identité de \(bridge.name) est prête pour l’association."
            isError = false
            return certificate
        } catch {
            report(error)
            return nil
        }
    }

    func pair(bridge: HueBridgeCandidate, certificatePEM: String) async {
        isPairing = true
        pairingSucceeded = false
        defer { isPairing = false }
        do {
            let result = try await client.pairHue(authority: bridge.host, certificatePEM: certificatePEM)
            message = "Bridge associé : \(result.discoveredDevices) appareil(s) découvert(s)."
            isError = false
            pairingSucceeded = true
            await load()
        } catch {
            report(error)
        }
    }

    private func report(_ error: Error) {
        message = error.localizedDescription
        isError = true
    }
}

private struct AutomationListView: View {
    @Environment(\.dismiss) private var dismiss
    @StateObject private var model: AutomationListModel
    #if os(macOS)
    @State private var showsGuidedCreator = false
    #endif

    init(client: RobineClient) {
        _model = StateObject(wrappedValue: AutomationListModel(client: client))
    }

    var body: some View {
        NavigationStack {
            Group {
                if model.automations.isEmpty && !model.isLoading {
                    ContentUnavailableView(
                        "Aucune habitude",
                        systemImage: "sparkles",
                        description: Text("Les habitudes Flow créées sur Robine apparaîtront ici.")
                    )
                } else {
                    List(model.automations) { automation in
                        Section {
                            VStack(alignment: .leading, spacing: 8) {
                                HStack {
                                    Image(systemName: automation.enabled ? "sparkles" : "pause.circle")
                                        .foregroundStyle(automation.enabled ? .mint : .secondary)
                                    Text(automation.enabled ? "Active" : "En pause")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                    Spacer()
                                    Text("v\(automation.revision)")
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                }
                                Text(automation.source)
                                    .font(.system(.footnote, design: .monospaced))
                                    .textSelection(.enabled)
                                    .lineLimit(8)
                                    .accessibilityLabel("Source Flow de \(automation.name)")
                                Button(model.simulatingID == automation.id ? "Simulation…" : "Simuler sans agir") {
                                    Task { await model.simulate(automation) }
                                }
                                .disabled(model.simulatingID != nil)
                                Button(model.loadingHistoryID == automation.id ? "Historique…" : "Historique") {
                                    Task { await model.loadHistory(automation) }
                                }
                                .disabled(model.loadingHistoryID != nil)
                                #if os(macOS)
                                NavigationLink("Modifier Flow") {
                                    AutomationEditorView(automation: automation, client: model.clientForEditing) { updated in
                                        model.replace(updated)
                                    }
                                }
                                #endif
                            }
                        } header: {
                            Text(automation.name)
                        }
                    }
                    .refreshable { await model.load() }
                }
            }
            .navigationTitle("Habitudes")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Terminé", action: { dismiss() })
                }
                #if os(macOS)
                ToolbarItem(placement: .primaryAction) {
                    Button("Nouvelle habitude") { showsGuidedCreator = true }
                }
                #endif
            }
            .overlay(alignment: .bottom) {
                if let error = model.error {
                    Text(error)
                        .font(.footnote)
                        .foregroundStyle(.red)
                        .padding()
                        .background(.thinMaterial)
                        .accessibilityLabel("Erreur : \(error)")
                }
            }
            .task { await model.load() }
            .sheet(item: $model.simulation) { presentation in
                AutomationSimulationView(presentation: presentation)
            }
            .sheet(item: $model.history) { presentation in
                AutomationHistoryView(presentation: presentation)
            }
            #if os(macOS)
            .sheet(isPresented: $showsGuidedCreator) {
                GuidedAutomationCreator(client: model.clientForEditing) { created in
                    model.insert(created)
                }
            }
            #endif
        }
    }
}

@MainActor
private final class AutomationListModel: ObservableObject {
    @Published private(set) var automations: [RobineAutomation] = []
    @Published private(set) var isLoading = false
    @Published private(set) var simulatingID: UUID?
    @Published var simulation: AutomationSimulationPresentation?
    @Published private(set) var loadingHistoryID: UUID?
    @Published var history: AutomationHistoryPresentation?
    @Published private(set) var error: String?

    private let client: RobineClient

    init(client: RobineClient) { self.client = client }

    var clientForEditing: RobineClient { client }

    func load() async {
        isLoading = true
        defer { isLoading = false }
        do {
            automations = try await client.automations()
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    func simulate(_ automation: RobineAutomation) async {
        simulatingID = automation.id
        defer { simulatingID = nil }
        do {
            let result = try await client.simulateAutomation(automation)
            simulation = AutomationSimulationPresentation(name: automation.name, simulation: result)
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    func loadHistory(_ automation: RobineAutomation) async {
        loadingHistoryID = automation.id
        defer { loadingHistoryID = nil }
        do {
            history = AutomationHistoryPresentation(
                name: automation.name,
                runs: try await client.automationRuns(automation)
            )
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    func replace(_ automation: RobineAutomation) {
        guard let index = automations.firstIndex(where: { $0.id == automation.id }) else { return }
        automations[index] = automation
    }

    func insert(_ automation: RobineAutomation) {
        automations.append(automation)
        automations.sort { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }
}

private struct AutomationSimulationPresentation: Identifiable {
    let id = UUID()
    let name: String
    let simulation: RobineFlowSimulation
}

private struct AutomationHistoryPresentation: Identifiable {
    let id = UUID()
    let name: String
    let runs: [RobineAutomationRun]
}

private struct AutomationSimulationView: View {
    @Environment(\.dismiss) private var dismiss
    let presentation: AutomationSimulationPresentation

    var body: some View {
        NavigationStack {
            List {
                Section("Résultat") {
                    LabeledContent("Habitude", value: presentation.name)
                    LabeledContent("État", value: simulationStatus)
                    LabeledContent("Commandes prévues", value: "\(presentation.simulation.commandCount)")
                }
                if !presentation.simulation.diagnostics.isEmpty {
                    Section("Diagnostics") {
                        ForEach(presentation.simulation.diagnostics) { diagnostic in
                            VStack(alignment: .leading) {
                                Text(diagnostic.message)
                                Text(diagnostic.code)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                }
                Section("Trace simulée") {
                    if presentation.simulation.result.steps.isEmpty {
                        Text("Aucune action n’est prévue.")
                            .foregroundStyle(.secondary)
                    } else {
                        ForEach(presentation.simulation.result.steps) { step in
                            Text(step.description)
                        }
                    }
                }
            }
            .navigationTitle("Simulation")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Terminé", action: { dismiss() })
                }
            }
        }
    }

    private var simulationStatus: String {
        switch presentation.simulation.result.status {
        case "completed": "Terminée sans effet"
        case "skipped": "Non déclenchée"
        case "suspended": "En attente simulée"
        case "queued": "En file simulée"
        default: presentation.simulation.result.status
        }
    }
}

private struct AutomationHistoryView: View {
    @Environment(\.dismiss) private var dismiss
    let presentation: AutomationHistoryPresentation

    var body: some View {
        NavigationStack {
            List {
                if presentation.runs.isEmpty {
                    ContentUnavailableView(
                        "Aucune exécution",
                        systemImage: "clock.arrow.circlepath",
                        description: Text("Robine affichera ici les prochaines habitudes exécutées.")
                    )
                }
                ForEach(presentation.runs) { run in
                    Section {
                        ForEach(Array(run.result.steps.enumerated()), id: \.offset) { _, step in
                            Text(step.description)
                                .font(.footnote)
                        }
                    } header: {
                        HStack {
                            Text(run.result.status == "completed" ? "Terminée" : run.result.status)
                            Spacer()
                            Text(run.recordedAt, style: .relative)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
            .navigationTitle("Historique · \(presentation.name)")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Terminé", action: { dismiss() })
                }
            }
        }
    }
}

#if os(macOS)
private struct AutomationEditorView: View {
    @Environment(\.dismiss) private var dismiss
    let automation: RobineAutomation
    let client: RobineClient
    let onSaved: (RobineAutomation) -> Void
    @State private var source: String
    @State private var enabled: Bool
    @State private var isSaving = false
    @State private var error: String?

    init(
        automation: RobineAutomation,
        client: RobineClient,
        onSaved: @escaping (RobineAutomation) -> Void
    ) {
        self.automation = automation
        self.client = client
        self.onSaved = onSaved
        _source = State(initialValue: automation.source)
        _enabled = State(initialValue: automation.enabled)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                VStack(alignment: .leading) {
                    Text(automation.name).font(.title2.bold())
                    Text("Flow est validé par le serveur avant enregistrement.")
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Toggle("Active", isOn: $enabled)
                    .toggleStyle(.switch)
            }
            TextEditor(text: $source)
                .font(.system(.body, design: .monospaced))
                .padding(8)
                .overlay(RoundedRectangle(cornerRadius: 8).stroke(.quaternary))
                .accessibilityLabel("Source Flow de \(automation.name)")
            if let error {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .accessibilityLabel("Erreur de validation : \(error)")
            }
            HStack {
                Spacer()
                Button("Annuler", role: .cancel, action: { dismiss() })
                Button(isSaving ? "Enregistrement…" : "Enregistrer") {
                    Task { await save() }
                }
                .buttonStyle(.borderedProminent)
                .disabled(isSaving || source.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(24)
        .frame(minWidth: 680, minHeight: 480)
        .navigationTitle("Éditer une habitude")
    }

    private func save() async {
        isSaving = true
        defer { isSaving = false }
        do {
            let updated = try await client.updateAutomation(automation, source: source, enabled: enabled)
            onSaved(updated)
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
#endif

#if os(macOS)
private struct GuidedAutomationCreator: View {
    @Environment(\.dismiss) private var dismiss
    let client: RobineClient
    let onCreated: (RobineAutomation) -> Void
    @State private var name = "Lumière douce"
    @State private var scheduledAt = Date()
    @State private var action = GuidedLightAction.turnOn
    @State private var brightness = 40.0
    @State private var enabled = true
    @State private var lights: [RobineEntity] = []
    @State private var selectedLightID: UUID?
    @State private var isLoading = false
    @State private var isSaving = false
    @State private var error: String?

    init(client: RobineClient, onCreated: @escaping (RobineAutomation) -> Void) {
        self.client = client
        self.onCreated = onCreated
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Quand") {
                    TextField("Nom de l’habitude", text: $name)
                    DatePicker("Chaque jour à", selection: $scheduledAt, displayedComponents: .hourAndMinute)
                    Text("Fuseau : \(TimeZone.current.identifier)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Section("Quoi") {
                    Picker("Lumière", selection: $selectedLightID) {
                        Text("Choisir une lumière").tag(Optional<UUID>.none)
                        ForEach(lights) { light in
                            Text(light.name).tag(Optional(light.id))
                        }
                    }
                    .disabled(isLoading || lights.isEmpty)
                    Picker("Action", selection: $action) {
                        ForEach(GuidedLightAction.allCases) { action in
                            Text(action.label).tag(action)
                        }
                    }
                    if action == .turnOn {
                        HStack {
                            Text("Luminosité")
                            Slider(value: $brightness, in: 1...100, step: 1)
                            Text("\(Int(brightness)) %")
                                .monospacedDigit()
                                .frame(width: 48, alignment: .trailing)
                        }
                    }
                    Toggle("Activer dès l’enregistrement", isOn: $enabled)
                }
                Section("Aperçu Flow") {
                    Text(flowSource ?? "Choisissez une lumière pour construire l’habitude.")
                        .font(.system(.footnote, design: .monospaced))
                        .textSelection(.enabled)
                        .foregroundStyle(flowSource == nil ? .secondary : .primary)
                }
                if let error {
                    Section {
                        Text(error)
                            .font(.footnote)
                            .foregroundStyle(.red)
                            .accessibilityLabel("Erreur : \(error)")
                    }
                }
            }
            .navigationTitle("Nouvelle habitude")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Annuler", action: { dismiss() })
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(isSaving ? "Enregistrement…" : "Créer") {
                        Task { await create() }
                    }
                    .disabled(isSaving || flowSource == nil)
                }
            }
            .task { await loadLights() }
        }
        .frame(minWidth: 560, minHeight: 460)
    }

    private var selectedLight: RobineEntity? {
        lights.first { $0.id == selectedLightID }
    }

    private var flowSource: String? {
        guard let light = selectedLight else { return nil }
        let time = Self.timeFormatter.string(from: scheduledAt)
        let name = Self.flowString(name)
        let command: String
        switch action {
        case .turnOn:
            command = "(command (entity \"\(light.id.uuidString)\") :turn-on :brightness \(Int(brightness))%)"
        case .turnOff:
            command = "(command (entity \"\(light.id.uuidString)\") :turn-off)"
        }
        return """
        (flow
          (meta :name \(name))
          (on (schedule :at \"\(time)\" :timezone \"\(TimeZone.current.identifier)\"))
          (do \(command)))
        """
    }

    private func loadLights() async {
        isLoading = true
        defer { isLoading = false }
        do {
            lights = try await client.devices()
                .flatMap(\.entities)
                .filter { entity in
                    entity.kind == "light" && entity.capabilities.contains(where: { $0.key == "switch" })
                }
                .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
            selectedLightID = lights.first?.id
            error = lights.isEmpty ? "Aucune lumière contrôlable n’est encore disponible." : nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func create() async {
        guard let source = flowSource else { return }
        isSaving = true
        defer { isSaving = false }
        do {
            let created = try await client.createAutomation(source: source, enabled: enabled)
            onCreated(created)
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }

    private static let timeFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "HH:mm"
        return formatter
    }()

    private static func flowString(_ value: String) -> String {
        let encoded = try? JSONEncoder().encode(value)
        return encoded.flatMap { String(data: $0, encoding: .utf8) } ?? "\"Nouvelle habitude\""
    }
}

private enum GuidedLightAction: String, CaseIterable, Identifiable {
    case turnOn, turnOff

    var id: String { rawValue }
    var label: String { self == .turnOn ? "Allumer" : "Éteindre" }
}
#endif

#if os(macOS)
private struct BackupPanel: View {
    @Environment(\.dismiss) private var dismiss
    @StateObject private var model: BackupModel

    init(client: RobineClient) {
        _model = StateObject(wrappedValue: BackupModel(client: client))
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Sauvegarde locale") {
                    Text("Robine crée un instantané SQLite cohérent sur le serveur. Les secrets du trousseau n’y figurent pas.")
                        .foregroundStyle(.secondary)
                    Button(model.isCreating ? "Création…" : "Créer une sauvegarde vérifiée") {
                        Task { await model.create() }
                    }
                    .disabled(model.isCreating)
                }
                if let backup = model.latestBackup {
                    Section("Dernière sauvegarde") {
                        LabeledContent("Fichier", value: backup.databaseFile)
                        LabeledContent("Créée", value: backup.createdAt)
                        LabeledContent("Taille", value: ByteCountFormatter.string(fromByteCount: Int64(backup.bytes), countStyle: .file))
                        LabeledContent("Intégrité", value: "SHA-256 \(String(backup.sha256.prefix(12)))…")
                    }
                }
                Section("Restauration") {
                    Text("La restauration requiert un mode maintenance : adaptateurs et écritures doivent être arrêtés proprement. Elle n’est donc pas proposée pendant que Robine sert la maison.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                if let error = model.error {
                    Section {
                        Text(error)
                            .font(.footnote)
                            .foregroundStyle(.red)
                            .accessibilityLabel("Erreur : \(error)")
                    }
                }
            }
            .navigationTitle("Sauvegarde")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Terminé", action: { dismiss() })
                }
            }
        }
        .frame(minWidth: 480, minHeight: 360)
    }
}

@MainActor
private final class BackupModel: ObservableObject {
    @Published private(set) var latestBackup: RobineBackup?
    @Published private(set) var isCreating = false
    @Published private(set) var error: String?
    private let client: RobineClient

    init(client: RobineClient) { self.client = client }

    func create() async {
        isCreating = true
        defer { isCreating = false }
        do {
            latestBackup = try await client.createBackup()
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }
}

private struct DiagnosticsPanel: View {
    @Environment(\.dismiss) private var dismiss
    @StateObject private var model: DiagnosticsModel

    init(client: RobineClient) {
        _model = StateObject(wrappedValue: DiagnosticsModel(client: client))
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("État de Robine") {
                    if let health = model.health {
                        LabeledContent("Serveur", value: health.status == "healthy" ? "Tout va bien" : "Attention requise")
                        LabeledContent("Base initialisée", value: health.initialized ? "Oui" : "Non")
                        if health.degradedAdapters.isEmpty {
                            Text("Tous les adaptateurs actifs répondent.")
                                .foregroundStyle(.secondary)
                        } else {
                            ForEach(health.degradedAdapters, id: \.self) { adapter in
                                Label(adapter, systemImage: "exclamationmark.triangle")
                                    .foregroundStyle(.orange)
                            }
                        }
                    } else if model.isLoading {
                        HStack { ProgressView().controlSize(.small); Text("Lecture de l’état…") }
                    } else {
                        Text("L’état du serveur est indisponible.")
                            .foregroundStyle(.secondary)
                    }
                    Button("Actualiser") { Task { await model.load() } }
                        .disabled(model.isLoading)
                }
                if !model.adapters.isEmpty {
                    Section("Adaptateurs") {
                        ForEach(model.adapters) { adapter in
                            VStack(alignment: .leading, spacing: 3) {
                                HStack {
                                    Image(systemName: adapter.status == "available" ? "checkmark.circle" : "exclamationmark.triangle")
                                        .foregroundStyle(adapter.status == "available" ? .mint : .orange)
                                        .accessibilityHidden(true)
                                    Text(adapter.adapterID)
                                    Spacer()
                                    Text(adapter.status)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                                if let detail = adapter.detail, !detail.isEmpty {
                                    Text(detail)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                                Text("Vu \(adapter.observedAt)")
                                    .font(.caption2)
                                    .foregroundStyle(.tertiary)
                            }
                            .accessibilityLabel("\(adapter.adapterID), \(adapter.status), \(adapter.detail ?? "sans détail")")
                        }
                    }
                }
                Section("Philips Hue") {
                    Text("Relit l’inventaire du bridge et remet les lumières découvertes à jour. Cette action ne change ni les pièces ni les automatismes.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                    Button(model.isSynchronizing ? "Resynchronisation…" : "Resynchroniser le bridge") {
                        Task { await model.synchronizeHue() }
                    }
                    .disabled(model.isSynchronizing)
                    if let synchronizedDevices = model.synchronizedDevices {
                        Text("\(synchronizedDevices) appareil(s) relu(s) lors de la dernière resynchronisation.")
                            .foregroundStyle(.secondary)
                    }
                }
                if let error = model.error {
                    Section {
                        Text(error)
                            .font(.footnote)
                            .foregroundStyle(.red)
                            .accessibilityLabel("Erreur : \(error)")
                    }
                }
            }
            .navigationTitle("Diagnostic")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Terminé", action: { dismiss() })
                }
            }
            .task { await model.load() }
        }
        .frame(minWidth: 480, minHeight: 380)
    }
}

@MainActor
private final class DiagnosticsModel: ObservableObject {
    @Published private(set) var health: RobineServerHealth?
    @Published private(set) var adapters: [RobineAdapterHealth] = []
    @Published private(set) var isLoading = false
    @Published private(set) var isSynchronizing = false
    @Published private(set) var synchronizedDevices: Int?
    @Published private(set) var error: String?
    private let client: RobineClient

    init(client: RobineClient) { self.client = client }

    func load() async {
        isLoading = true
        defer { isLoading = false }
        do {
            async let fetchedHealth = client.health()
            async let fetchedAdapters = client.adapters()
            health = try await fetchedHealth
            adapters = try await fetchedAdapters
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    func synchronizeHue() async {
        isSynchronizing = true
        defer { isSynchronizing = false }
        do {
            let result = try await client.synchronizeHue()
            synchronizedDevices = result.discoveredDevices
            error = nil
            await load()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
#endif
