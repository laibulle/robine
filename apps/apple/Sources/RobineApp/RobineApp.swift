import RobineClient
import RobineUI
import SwiftUI

@main
struct RobineApp: App {
    @StateObject private var connection = ConnectionModel()

    var body: some Scene {
        WindowGroup {
            if let client = connection.client {
                HomeView(client: client)
                    .environmentObject(connection)
            } else {
                ConnectionView(connection: connection)
            }
        }
    }
}

@MainActor
private final class ConnectionModel: ObservableObject {
    @Published var address: String
    @Published var token = ""
    @Published var error: String?
    @Published private(set) var client: RobineClient?

    private let defaults = UserDefaults.standard
    private let tokenStore = KeychainTokenStore()
    private static let addressKey = "io.robine.apple.server-address"

    init() {
        address = defaults.string(forKey: Self.addressKey) ?? "https://robine.local"
        restoreSavedConnection()
    }

    func connect() {
        error = nil
        guard let url = normalizedURL(from: address) else {
            error = "Saisissez une adresse HTTPS Robine valide."
            return
        }
        let server = RobineServer(baseURL: url)
        do {
            if !token.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                try tokenStore.save(token.trimmingCharacters(in: .whitespacesAndNewlines), for: server)
            }
            guard try tokenStore.token(for: server) != nil else {
                error = "Collez le jeton local fourni lors de l’association."
                return
            }
            defaults.set(url.absoluteString, forKey: Self.addressKey)
            client = RobineClient(server: server, tokenStore: tokenStore)
            token = ""
        } catch {
            self.error = error.localizedDescription
        }
    }

    func forget() {
        guard let url = normalizedURL(from: address) else {
            client = nil
            return
        }
        do {
            try tokenStore.deleteToken(for: RobineServer(baseURL: url))
        } catch {
            self.error = error.localizedDescription
        }
        client = nil
        token = ""
    }

    private func restoreSavedConnection() {
        guard let url = normalizedURL(from: address) else { return }
        let server = RobineServer(baseURL: url)
        if (try? tokenStore.token(for: server)) != nil {
            client = RobineClient(server: server, tokenStore: tokenStore)
        }
    }

    private func normalizedURL(from value: String) -> URL? {
        guard var components = URLComponents(string: value.trimmingCharacters(in: .whitespacesAndNewlines)),
              let scheme = components.scheme?.lowercased(),
              let host = components.host,
              !host.isEmpty else { return nil }
        let isLoopback = host == "localhost" || host == "127.0.0.1" || host == "::1"
        guard scheme == "https" || (scheme == "http" && isLoopback) else { return nil }
        components.path = components.path.isEmpty ? "/" : components.path
        return components.url
    }
}

private struct ConnectionView: View {
    @ObservedObject var connection: ConnectionModel

    var body: some View {
        VStack(spacing: 20) {
            Image(systemName: "pawprint.fill")
                .font(.system(size: 48))
                .foregroundStyle(.mint)
                .accessibilityHidden(true)
            Text("Retrouver Robine")
                .font(.title.bold())
            Text("Associez cette app à votre serveur local. Le jeton reste dans le trousseau de l’appareil.")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            TextField("https://robine.local", text: $connection.address)
                .textFieldStyle(.roundedBorder)
            SecureField("Jeton local", text: $connection.token)
                .textFieldStyle(.roundedBorder)
            if let error = connection.error {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .accessibilityLabel("Erreur : \(error)")
            }
            Button("Se connecter", action: connection.connect)
                .buttonStyle(.borderedProminent)
            Button("Oublier cette association", role: .destructive, action: connection.forget)
                .font(.footnote)
        }
        .padding(32)
        .frame(maxWidth: 440)
    }
}
