import SwiftUI
import RobineClient

@MainActor
public final class HomeViewModel: ObservableObject {
    @Published public private(set) var devices: [RobineDevice] = []
    @Published public private(set) var message = "Robine veille tranquillement sur la maison."
    private let client: RobineClient

    public init(client: RobineClient) { self.client = client }
    public func refresh() async {
        do { devices = try await client.devices(); message = "La maison est à jour." }
        catch { message = error.localizedDescription }
    }
}

public struct HomeView: View {
    @StateObject private var model: HomeViewModel
    public init(client: RobineClient) { _model = StateObject(wrappedValue: HomeViewModel(client: client)) }
    public var body: some View {
        NavigationStack {
            List(model.devices) { device in
                Section(device.name) {
                    ForEach(device.entities) { entity in
                        HStack { Text(entity.name); Spacer(); Text(entity.kind).foregroundStyle(.secondary) }
                    }
                }
            }
            .navigationTitle("Robine")
            .safeAreaInset(edge: .bottom) { Text(model.message).font(.footnote).foregroundStyle(.secondary).padding() }
            .task { await model.refresh() }
            .refreshable { await model.refresh() }
        }
    }
}
