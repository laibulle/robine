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
    @Published private(set) var pendingServerCertificate: RobineServerCertificate?

    private let defaults = UserDefaults.standard
    private let tokenStore = KeychainTokenStore()
    private let trustStore = KeychainServerTrustStore()
    private static let addressKey = "io.robine.apple.server-address"
    private var pendingServer: RobineServer?
    private var pendingToken = ""

    init() {
        address = defaults.string(forKey: Self.addressKey) ?? "https://robine.local"
        restoreSavedConnection()
    }

    func connect() {
        Task { await prepareConnection() }
    }

    func confirmServerCertificate() {
        guard let server = pendingServer, let certificate = pendingServerCertificate else { return }
        do {
            try trustStore.saveCertificateSHA256(certificate.sha256, for: server)
            finishConnection(server: server, certificateSHA256: certificate.sha256, suppliedToken: pendingToken)
        } catch {
            self.error = error.localizedDescription
        }
    }

    func cancelServerCertificateConfirmation() {
        pendingServer = nil
        pendingToken = ""
        pendingServerCertificate = nil
    }

    private func prepareConnection() async {
        error = nil
        pendingServer = nil
        pendingToken = ""
        pendingServerCertificate = nil
        guard let url = normalizedURL(from: address) else {
            error = "Saisissez une adresse HTTPS Robine valide."
            return
        }
        let server = RobineServer(baseURL: url)
        do {
            let suppliedToken = token.trimmingCharacters(in: .whitespacesAndNewlines)
            let savedToken = try tokenStore.token(for: server)
            guard !suppliedToken.isEmpty || savedToken != nil else {
                error = "Collez le jeton local fourni lors de l’association."
                return
            }
            if url.scheme?.lowercased() == "https" {
                if let certificateSHA256 = try trustStore.certificateSHA256(for: server) {
                    finishConnection(
                        server: server,
                        certificateSHA256: certificateSHA256,
                        suppliedToken: suppliedToken
                    )
                    return
                }
                pendingServer = server
                pendingToken = suppliedToken
                pendingServerCertificate = try await RobineServerCertificateProbe.probe(server: server)
                return
            }
            finishConnection(server: server, certificateSHA256: nil, suppliedToken: suppliedToken)
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func finishConnection(
        server: RobineServer,
        certificateSHA256: String?,
        suppliedToken: String
    ) {
        do {
            if !suppliedToken.isEmpty {
                try tokenStore.save(suppliedToken, for: server)
            }
            guard try tokenStore.token(for: server) != nil else {
                error = "Collez le jeton local fourni lors de l’association."
                return
            }
            defaults.set(server.baseURL.absoluteString, forKey: Self.addressKey)
            client = if let certificateSHA256 {
                RobineClient(
                    server: server,
                    pinnedCertificateSHA256: certificateSHA256,
                    tokenStore: tokenStore
                )
            } else {
                RobineClient(server: server, tokenStore: tokenStore)
            }
            token = ""
            pendingServer = nil
            pendingToken = ""
            pendingServerCertificate = nil
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
            let server = RobineServer(baseURL: url)
            try tokenStore.deleteToken(for: server)
            try trustStore.deleteCertificateSHA256(for: server)
        } catch {
            self.error = error.localizedDescription
        }
        client = nil
        token = ""
    }

    private func restoreSavedConnection() {
        guard let url = normalizedURL(from: address) else { return }
        let server = RobineServer(baseURL: url)
        guard (try? tokenStore.token(for: server)) != nil else { return }
        if url.scheme?.lowercased() == "https" {
            guard let certificateSHA256 = try? trustStore.certificateSHA256(for: server) else { return }
            client = RobineClient(
                server: server,
                pinnedCertificateSHA256: certificateSHA256,
                tokenStore: tokenStore
            )
        } else {
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
            if let certificate = connection.pendingServerCertificate {
                VStack(alignment: .leading, spacing: 8) {
                    Label("Identité TLS du serveur", systemImage: "checkmark.shield")
                        .font(.headline)
                    Text("Empreinte observée : \(certificate.shortFingerprint)")
                        .font(.footnote.monospaced())
                    Text("Vérifiez cette empreinte auprès de la personne qui héberge Robine, puis confirmez pour l’épingler dans le trousseau de cet appareil.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                    HStack {
                        Button("Faire confiance à ce serveur", action: connection.confirmServerCertificate)
                            .buttonStyle(.borderedProminent)
                        Button("Annuler", role: .cancel, action: connection.cancelServerCertificateConfirmation)
                    }
                }
                .padding()
                .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 12))
            }
            if let error = connection.error {
                Text(error)
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .accessibilityLabel("Erreur : \(error)")
            }
            Button("Se connecter", action: connection.connect)
                .buttonStyle(.borderedProminent)
                .disabled(connection.pendingServerCertificate != nil)
            Button("Oublier cette association", role: .destructive, action: connection.forget)
                .font(.footnote)
        }
        .padding(32)
        .frame(maxWidth: 440)
    }
}
