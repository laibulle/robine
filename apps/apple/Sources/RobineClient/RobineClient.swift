import Foundation
import CryptoKit
import Security

public struct RobineServer: Codable, Sendable, Hashable {
    public let baseURL: URL
    public init(baseURL: URL) { self.baseURL = baseURL }
}

/// Cache de présentation non sensible. Les identifiants restent dans le
/// trousseau ; ce cache ne contient que les données déjà affichables hors ligne.
public protocol RobinePresentationStore: Sendable {
    func load(for server: RobineServer) throws -> RobinePresentationSnapshot?
    func save(_ snapshot: RobinePresentationSnapshot, for server: RobineServer) throws
    func delete(for server: RobineServer) throws
}

public struct RobinePresentationSnapshot: Codable, Sendable {
    public let cursor: UInt64
    public let devices: [RobineDevice]
    public let areas: [RobineArea]
    public let states: [RobineStateProperty]
    public let automations: [RobineAutomation]
    public let recentEvents: [RobineEvent]

    public init(
        cursor: UInt64,
        devices: [RobineDevice],
        areas: [RobineArea],
        states: [RobineStateProperty],
        automations: [RobineAutomation] = [],
        recentEvents: [RobineEvent] = []
    ) {
        self.cursor = cursor
        self.devices = devices
        self.areas = areas
        self.states = states
        self.automations = automations
        self.recentEvents = recentEvents
    }

    private enum CodingKeys: String, CodingKey {
        case cursor, devices, areas, states, automations, recentEvents
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        cursor = try values.decode(UInt64.self, forKey: .cursor)
        devices = try values.decode([RobineDevice].self, forKey: .devices)
        areas = try values.decode([RobineArea].self, forKey: .areas)
        states = try values.decode([RobineStateProperty].self, forKey: .states)
        automations = try values.decodeIfPresent([RobineAutomation].self, forKey: .automations) ?? []
        recentEvents = try values.decodeIfPresent([RobineEvent].self, forKey: .recentEvents) ?? []
    }
}

public final class UserDefaultsPresentationStore: RobinePresentationStore, @unchecked Sendable {
    private let defaults: UserDefaults

    public init(defaults: UserDefaults = .standard) { self.defaults = defaults }

    public func load(for server: RobineServer) throws -> RobinePresentationSnapshot? {
        guard let data = defaults.data(forKey: key(for: server)) else { return nil }
        return try JSONDecoder.robine.decode(RobinePresentationSnapshot.self, from: data)
    }

    public func save(_ snapshot: RobinePresentationSnapshot, for server: RobineServer) throws {
        defaults.set(try JSONEncoder.robine.encode(snapshot), forKey: key(for: server))
    }

    public func delete(for server: RobineServer) throws {
        defaults.removeObject(forKey: key(for: server))
    }

    private func key(for server: RobineServer) -> String {
        "io.robine.presentation." + Data(server.baseURL.absoluteString.utf8).base64EncodedString()
    }
}

/// Jetons de session locaux, isolés du cache de présentation et des URL.
public protocol RobineTokenStore: Sendable {
    func token(for server: RobineServer) throws -> String?
    func save(_ token: String, for server: RobineServer) throws
    func deleteToken(for server: RobineServer) throws
}

public struct KeychainTokenStore: RobineTokenStore {
    private let service: String
    public init(service: String = "io.robine.apple") { self.service = service }

    public func token(for server: RobineServer) throws -> String? {
        var query = baseQuery(server)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data, let token = String(data: data, encoding: .utf8) else { throw KeychainError(status) }
        return token
    }

    public func save(_ token: String, for server: RobineServer) throws {
        guard !token.isEmpty else { throw RobineClientError.missingToken }
        let query = baseQuery(server)
        let attributes = [kSecValueData as String: Data(token.utf8)]
        let status = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        if status == errSecSuccess { return }
        guard status == errSecItemNotFound else { throw KeychainError(status) }
        var item = query
        item[kSecValueData as String] = Data(token.utf8)
        let insertion = SecItemAdd(item as CFDictionary, nil)
        guard insertion == errSecSuccess else { throw KeychainError(insertion) }
    }

    public func deleteToken(for server: RobineServer) throws {
        let status = SecItemDelete(baseQuery(server) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw KeychainError(status) }
    }

    private func baseQuery(_ server: RobineServer) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: server.baseURL.absoluteString]
    }
}

/// Empreinte publique mais sensible : elle ancre l'identité TLS d'un serveur
/// local. Elle reste dans le trousseau afin qu'un cache de présentation ne
/// puisse jamais élargir silencieusement la confiance.
public protocol RobineServerTrustStore: Sendable {
    func certificateSHA256(for server: RobineServer) throws -> String?
    func saveCertificateSHA256(_ fingerprint: String, for server: RobineServer) throws
    func deleteCertificateSHA256(for server: RobineServer) throws
}

public struct KeychainServerTrustStore: RobineServerTrustStore {
    private let service: String

    public init(service: String = "io.robine.apple.server-trust") { self.service = service }

    public func certificateSHA256(for server: RobineServer) throws -> String? {
        var query = baseQuery(server)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess,
              let data = result as? Data,
              let fingerprint = String(data: data, encoding: .utf8) else {
            throw KeychainError(status)
        }
        return fingerprint
    }

    public func saveCertificateSHA256(_ fingerprint: String, for server: RobineServer) throws {
        let normalized = fingerprint.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard normalized.count == 64, normalized.allSatisfy({ $0.isHexDigit }) else {
            throw RobineClientError.invalidServerCertificate
        }
        let query = baseQuery(server)
        let attributes = [kSecValueData as String: Data(normalized.utf8)]
        let status = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        if status == errSecSuccess { return }
        guard status == errSecItemNotFound else { throw KeychainError(status) }
        var item = query
        item[kSecValueData as String] = Data(normalized.utf8)
        let insertion = SecItemAdd(item as CFDictionary, nil)
        guard insertion == errSecSuccess else { throw KeychainError(insertion) }
    }

    public func deleteCertificateSHA256(for server: RobineServer) throws {
        let status = SecItemDelete(baseQuery(server) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw KeychainError(status) }
    }

    private func baseQuery(_ server: RobineServer) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: server.baseURL.absoluteString,
        ]
    }
}

public struct KeychainError: LocalizedError {
    public let status: OSStatus
    public init(_ status: OSStatus) { self.status = status }
    public var errorDescription: String? { "Le trousseau Robine n’est pas disponible (code \(status))." }
}

public struct RobineDevice: Codable, Identifiable, Sendable {
    public let id: UUID
    public let name: String
    public let status: String
    public let entities: [RobineEntity]
}

private struct RobineDevicePage: Codable, Sendable {
    let devices: [RobineDevice]
    let nextCursor: String?
}

public struct RobineEntity: Codable, Identifiable, Sendable {
    public let id: UUID
    public let name: String
    public let kind: String
    public let areaId: UUID?
    public let capabilities: [RobineCapability]
}

public struct RobineEntityDetail: Codable, Sendable {
    public let entity: RobineEntity
    public let state: [RobineStateProperty]
}

public struct RobineStateProperty: Codable, Sendable {
    public let entityId: UUID
    public let key: String
    public let value: RobineStateValue
    public let quality: String
    public let sourceAt: Date
    public let receivedAt: Date
    public let version: UInt64
}

public enum RobineStateValue: Codable, Equatable, Sendable {
    case bool(Bool)
    case percentage(Double)
    case text(String)

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let value = try? container.decode(Bool.self) { self = .bool(value) }
        else if let value = try? container.decode(Double.self) { self = .percentage(value) }
        else { self = .text(try container.decode(String.self)) }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case let .bool(value): try container.encode(value)
        case let .percentage(value): try container.encode(value)
        case let .text(value): try container.encode(value)
        }
    }
}

public struct RobineCapability: Codable, Hashable, Sendable {
    public let key: String
    public let version: UInt16
}

public struct RobineArea: Codable, Identifiable, Sendable {
    public let id: UUID
    public let name: String
}

public struct RobineAutomation: Codable, Identifiable, Sendable {
    public let id: UUID
    public let name: String
    public let enabled: Bool
    public let revision: UInt64
    public let source: String
}

public struct RobineAutomationRun: Decodable, Identifiable, Sendable {
    public let id: UUID
    public let flowID: UUID
    public let recordedAt: Date
    public let result: RobineFlowSimulationResult

    private enum CodingKeys: String, CodingKey {
        case id
        // `convertFromSnakeCase` transforme `flow_id` en `flowId`.
        case flowID = "flowId"
        case recordedAt, result
    }
}

public struct RobineFlowSimulation: Decodable, Sendable {
    public let name: String?
    public let commandCount: Int
    public let diagnostics: [RobineFlowDiagnostic]
    public let result: RobineFlowSimulationResult
}

public struct RobineFlowDiagnostic: Decodable, Identifiable, Sendable {
    public let code: String
    public let message: String
    public var id: String { code + message }
}

public struct RobineFlowSimulationResult: Decodable, Sendable {
    public let status: String
    public let steps: [RobineFlowTraceStep]

    private enum CodingKeys: String, CodingKey {
        case status, steps, trace
    }

    private struct Trace: Decodable {
        let steps: [RobineFlowTraceStep]
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        status = try values.decode(String.self, forKey: .status)
        if let direct = try values.decodeIfPresent([RobineFlowTraceStep].self, forKey: .steps) {
            steps = direct
        } else {
            steps = try values.decodeIfPresent(Trace.self, forKey: .trace)?.steps ?? []
        }
    }
}

public struct RobineFlowTraceStep: Decodable, Identifiable, Sendable {
    public let type: String
    public let summary: String?
    public let message: String?
    public let entityID: String?
    public let verb: String?
    public let nextAttempt: UInt8?
    public let totalAttempts: UInt8?
    public let backoffMilliseconds: UInt64?
    public let attempts: UInt8?
    public let action: String?

    public var id: String { [type, summary, message, entityID, verb, action, nextAttempt.map(String.init), attempts.map(String.init)].compactMap { $0 }.joined(separator: ":") }

    public var description: String {
        if let summary { return summary }
        if let message { return message }
        if let entityID, let verb { return "Commande \(verb) demandée à \(entityID)." }
        switch type {
        case "waiting": return "Attente planifiée."
        case "awaiting": return "En attente d’un événement."
        case "automation_changed": return "État d’une habitude modifié."
        case "retry_scheduled":
            let attempt = nextAttempt.map(String.init) ?? "suivante"
            let total = totalAttempts.map(String.init) ?? "?"
            let delay = backoffMilliseconds.map { " après \($0) ms" } ?? ""
            return "Nouvel essai \(attempt)/\(total) programmé\(delay)."
        case "retry_exhausted": return "Les \(attempts.map(String.init) ?? "") tentatives ont échoué."
        case "action_failed": return action ?? "L’action n’a pas abouti."
        default: return type
        }
    }
}

public struct RobineEvent: Codable, Identifiable, Sendable {
    public let id: UInt64
    public let topic: String
    public let eventType: String
    public let occurredAt: Date
}

public struct RobineEventPage: Codable, Sendable {
    public let events: [RobineEvent]
    public let nextCursor: UInt64
}

public struct RobineBackup: Decodable, Sendable {
    public let manifestVersion: UInt16
    public let createdAt: String
    public let databaseFile: String
    public let bytes: UInt64
    public let sha256: String
}

public struct RobineServerHealth: Decodable, Sendable {
    public let status: String
    public let initialized: Bool
    public let degradedAdapters: [String]
}

public struct HueSynchronization: Decodable, Sendable {
    public let discoveredDevices: Int
}

public struct RobineAdapterHealth: Decodable, Identifiable, Sendable {
    public let adapterID: String
    public let status: String
    public let detail: String?
    public let observedAt: String
    public var id: String { adapterID }

    private enum CodingKeys: String, CodingKey {
        // `JSONDecoder.convertFromSnakeCase` transforme `adapter_id` en
        // `adapterId`; l'acronyme public `ID` réclame donc cette clé explicite.
        case adapterID = "adapterId"
        case status, detail
        case observedAt
    }
}

private struct AutomationUpdateRequest: Encodable {
    let source: String
    let enabled: Bool
}

private struct RobineServerError: Decodable {
    let message: String
}

public struct HueBridgeCandidate: Codable, Identifiable, Sendable {
    public let name: String
    public let host: String
    public let addresses: [String]
    public var id: String { host }

    public init(name: String, host: String, addresses: [String]) {
        self.name = name
        self.host = host
        self.addresses = addresses
    }
}

public struct HuePairingResult: Codable, Sendable {
    public let adapterId: String
    public let discoveredDevices: Int
}

/// Jeton d'accès MCP affiché une seule fois par le serveur. L'app ne le
/// conserve jamais : le client MCP choisi par l'administrateur en devient le
/// seul détenteur.
public struct RobineMcpToken: Codable, Sendable {
    public let token: String
    public let tokenId: String
    public let expiresAt: String
    public let scopes: [String]
}

/// Proposition d'import issue de l'inventaire Hue. Elle contient uniquement
/// les identifiants Robine des lumières, jamais les ressources Hue internes.
public struct HueRoomSuggestion: Codable, Identifiable, Sendable, Equatable {
    public let name: String
    public let entityIds: [UUID]
    public var id: String { name + ":" + entityIds.map(\.uuidString).sorted().joined(separator: ":") }
}

/// Certificat observé pendant la toute première connexion à un bridge choisi
/// par l'utilisateur dans la découverte locale. Il ne sert qu'à construire
/// l'épinglage côté serveur : il n'est jamais placé dans le cache de l'app.
public struct HueBridgeCertificate: Sendable, Equatable {
    public let pem: String
    public let sha256: String

    public var shortFingerprint: String {
        Array(sha256.prefix(16)).enumerated().reduce(into: "") { abbreviated, character in
            if character.offset > 0 && character.offset.isMultiple(of: 4) {
                abbreviated.append(":")
            }
            abbreviated.append(character.element)
        }
    }

    static func fromDER(_ der: Data) -> HueBridgeCertificate {
        let body = der.base64EncodedString(options: Data.Base64EncodingOptions.lineLength64Characters)
        let pem = "-----BEGIN CERTIFICATE-----\n\(body)\n-----END CERTIFICATE-----\n"
        let digest = SHA256.hash(data: Data(pem.utf8))
        let sha256 = digest.map { String(format: "%02x", $0) }.joined()
        return HueBridgeCertificate(pem: pem, sha256: sha256)
    }
}

public struct CommandAccepted: Codable, Sendable {
    public let commandId: UUID
    public let correlationId: String
}

public enum RobineClientError: LocalizedError {
    case missingToken, invalidResponse, unauthorized, server(String), invalidHueBridge, hueCertificateUnavailable, invalidServerCertificate, serverCertificateUnavailable
    public var errorDescription: String? {
        switch self {
        case .missingToken: "La connexion à Robine doit être associée à un accès local."
        case .invalidResponse: "La réponse de Robine est invalide."
        case .unauthorized: "L’accès local Robine a expiré ou n’est plus autorisé."
        case let .server(message): message
        case .invalidHueBridge: "L’adresse du bridge Hue n’est pas valide."
        case .hueCertificateUnavailable: "Le bridge n’a pas présenté de certificat TLS exploitable."
        case .invalidServerCertificate: "L’empreinte TLS Robine est invalide."
        case .serverCertificateUnavailable: "Le serveur Robine n’a pas présenté de certificat TLS exploitable."
        }
    }
}

/// Identité TLS vue lors de l'association initiale au serveur Robine. L'app
/// affiche son empreinte avant de l'épingler, y compris avec une PKI locale.
public struct RobineServerCertificate: Sendable, Equatable {
    public let sha256: String

    public var shortFingerprint: String {
        Array(sha256.prefix(16)).enumerated().reduce(into: "") { abbreviated, character in
            if character.offset > 0 && character.offset.isMultiple(of: 4) {
                abbreviated.append(":")
            }
            abbreviated.append(character.element)
        }
    }

    static func fromDER(_ der: Data) -> RobineServerCertificate {
        let digest = SHA256.hash(data: der)
        return RobineServerCertificate(
            sha256: digest.map { String(format: "%02x", $0) }.joined()
        )
    }
}

public enum RobineServerCertificateProbe {
    public static func probe(server: RobineServer) async throws -> RobineServerCertificate {
        guard server.baseURL.scheme?.lowercased() == "https",
              server.baseURL.host != nil,
              server.baseURL.user == nil,
              server.baseURL.password == nil else {
            throw RobineClientError.invalidResponse
        }
        let url = server.baseURL.appending(path: "health")
        return RobineServerCertificate.fromDER(try await probeCertificateDER(at: url))
    }
}

/// Lit le certificat présenté par un bridge local, uniquement pendant
/// l'association. Le challenge TLS est accepté une seule fois pour cette
/// requête éphémère et sans suivre de redirection ; après l'appui physique sur
/// le bridge, le serveur épingle ce même certificat pour tous les accès futurs.
public enum HueBridgeCertificateProbe {
    public static func probe(authority: String) async throws -> HueBridgeCertificate {
        let authority = authority.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URL(string: "https://\(authority)/clip/v2/resource/bridge"),
              url.scheme == "https",
              url.host != nil,
              url.user == nil,
              url.password == nil else {
            throw RobineClientError.invalidHueBridge
        }

        do {
            return HueBridgeCertificate.fromDER(try await probeCertificateDER(at: url))
        } catch RobineClientError.serverCertificateUnavailable {
            throw RobineClientError.hueCertificateUnavailable
        }
    }
}

private func probeCertificateDER(at url: URL) async throws -> Data {
    let delegate = CertificateProbeDelegate()
    let configuration = URLSessionConfiguration.ephemeral
    configuration.timeoutIntervalForRequest = 5
    configuration.timeoutIntervalForResource = 5
    let session = URLSession(configuration: configuration, delegate: delegate, delegateQueue: nil)
    defer { session.invalidateAndCancel() }
    _ = try await session.data(from: url)
    return try delegate.certificateDER()
}

private final class CertificateProbeDelegate: NSObject, URLSessionDelegate, URLSessionTaskDelegate, @unchecked Sendable {
    private let lock = NSLock()
    private var observedCertificateDER: Data?

    func urlSession(
        _ session: URLSession,
        didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @escaping @Sendable (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
              let trust = challenge.protectionSpace.serverTrust,
              let certificates = SecTrustCopyCertificateChain(trust) as? [SecCertificate],
              let certificate = certificates.first else {
            completionHandler(.performDefaultHandling, nil)
            return
        }
        lock.lock()
        observedCertificateDER = SecCertificateCopyData(certificate) as Data
        lock.unlock()
        // Cette exception ne s'applique qu'à cette requête éphémère, vers le
        // bridge choisi. Les clients Robine ordinaires gardent la validation
        // TLS système et le serveur utilisera ensuite le certificat épinglé.
        completionHandler(.useCredential, URLCredential(trust: trust))
    }

    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest,
        completionHandler: @escaping @Sendable (URLRequest?) -> Void
    ) {
        completionHandler(nil)
    }

    func certificateDER() throws -> Data {
        lock.lock()
        let der = observedCertificateDER
        lock.unlock()
        guard let der else { throw RobineClientError.serverCertificateUnavailable }
        return der
    }
}

private final class RobineServerTrustDelegate: NSObject, URLSessionDelegate, URLSessionTaskDelegate, @unchecked Sendable {
    private let host: String
    private let certificateSHA256: String

    init(server: RobineServer, certificateSHA256: String) {
        host = server.baseURL.host?.lowercased() ?? ""
        self.certificateSHA256 = certificateSHA256.lowercased()
    }

    func urlSession(
        _ session: URLSession,
        didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @escaping @Sendable (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
              challenge.protectionSpace.host.lowercased() == host,
              let trust = challenge.protectionSpace.serverTrust,
              let certificates = SecTrustCopyCertificateChain(trust) as? [SecCertificate],
              let certificate = certificates.first else {
            completionHandler(.cancelAuthenticationChallenge, nil)
            return
        }
        let der = SecCertificateCopyData(certificate) as Data
        let presented = RobineServerCertificate.fromDER(der).sha256
        guard presented == certificateSHA256 else {
            completionHandler(.cancelAuthenticationChallenge, nil)
            return
        }
        completionHandler(.useCredential, URLCredential(trust: trust))
    }

    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest,
        completionHandler: @escaping @Sendable (URLRequest?) -> Void
    ) {
        // Les endpoints Robine ne redirigent pas. Refuser évite qu'un bearer
        // soit rejoué hors de l'origine explicitement épinglée.
        completionHandler(nil)
    }
}

public actor RobineClient {
    public nonisolated let server: RobineServer
    private let session: URLSession
    private let tokenProvider: @Sendable () throws -> String

    public init(server: RobineServer, session: URLSession = .shared, tokenProvider: @escaping @Sendable () throws -> String) {
        self.server = server
        self.session = session
        self.tokenProvider = tokenProvider
    }

    public init(server: RobineServer, session: URLSession = .shared, tokenStore: any RobineTokenStore) {
        self.init(server: server, session: session) {
            guard let token = try tokenStore.token(for: server) else { throw RobineClientError.missingToken }
            return token
        }
    }

    /// Les serveurs HTTPS locaux peuvent employer une PKI domestique. Après
    /// confirmation humaine, l'empreinte est comparée à chaque challenge TLS,
    /// y compris pour le WebSocket, sans désactiver la validation globalement.
    public init(
        server: RobineServer,
        pinnedCertificateSHA256: String,
        tokenStore: any RobineTokenStore
    ) {
        let configuration = URLSessionConfiguration.default
        let delegate = RobineServerTrustDelegate(
            server: server,
            certificateSHA256: pinnedCertificateSHA256
        )
        let session = URLSession(configuration: configuration, delegate: delegate, delegateQueue: nil)
        self.init(server: server, session: session, tokenStore: tokenStore)
    }

    public func devices() async throws -> [RobineDevice] {
        var devices = [RobineDevice]()
        var cursor: String?
        repeat {
            var components = URLComponents(url: server.baseURL.appending(path: "api/v1/devices"), resolvingAgainstBaseURL: false)
            components?.queryItems = [URLQueryItem(name: "limit", value: "100")]
                + (cursor.map { [URLQueryItem(name: "cursor", value: $0)] } ?? [])
            guard let url = components?.url else { throw RobineClientError.invalidResponse }
            let page: RobineDevicePage = try await request(url: url, method: "GET")
            devices.append(contentsOf: page.devices)
            cursor = page.nextCursor
        } while cursor != nil
        return devices
    }

    public func areas() async throws -> [RobineArea] {
        try await request(path: "api/v1/areas", method: "GET")
    }

    public func automations() async throws -> [RobineAutomation] {
        try await request(path: "api/v1/automations", method: "GET")
    }

    public func automationRuns(_ automation: RobineAutomation, limit: Int = 20) async throws -> [RobineAutomationRun] {
        guard (1...100).contains(limit) else { throw RobineClientError.invalidResponse }
        var components = URLComponents(
            url: server.baseURL.appending(path: "api/v1/automations/\(automation.id.uuidString)/runs"),
            resolvingAgainstBaseURL: false
        )
        components?.queryItems = [URLQueryItem(name: "limit", value: String(limit))]
        guard let url = components?.url else { throw RobineClientError.invalidResponse }
        return try await request(url: url, method: "GET")
    }

    public func createAutomation(source: String, enabled: Bool = true) async throws -> RobineAutomation {
        try await request(
            path: "api/v1/automations",
            method: "POST",
            body: try JSONEncoder().encode(AutomationUpdateRequest(source: source, enabled: enabled))
        )
    }

    public func updateAutomation(
        _ automation: RobineAutomation,
        enabled: Bool
    ) async throws -> RobineAutomation {
        try await updateAutomation(automation, source: automation.source, enabled: enabled)
    }

    public func updateAutomation(
        _ automation: RobineAutomation,
        source: String,
        enabled: Bool
    ) async throws -> RobineAutomation {
        try await request(
            path: "api/v1/automations/\(automation.id.uuidString)",
            method: "PATCH",
            body: try JSONEncoder().encode(AutomationUpdateRequest(source: source, enabled: enabled))
        )
    }

    public func simulateAutomation(_ automation: RobineAutomation) async throws -> RobineFlowSimulation {
        try await request(
            path: "api/v1/automations/\(automation.id.uuidString)/simulate",
            method: "POST"
        )
    }

    public func recentEvents(limit: UInt = 20) async throws -> RobineEventPage {
        guard (1...500).contains(limit) else { throw RobineClientError.invalidResponse }
        var components = URLComponents(url: server.baseURL.appending(path: "api/v1/events"), resolvingAgainstBaseURL: false)
        components?.queryItems = [URLQueryItem(name: "tail", value: String(limit))]
        guard let url = components?.url else { throw RobineClientError.invalidResponse }
        return try await request(url: url, method: "GET")
    }

    public func createBackup() async throws -> RobineBackup {
        try await request(path: "api/v1/backups", method: "POST")
    }

    public func health() async throws -> RobineServerHealth {
        try await request(path: "health", method: "GET")
    }

    public func adapters() async throws -> [RobineAdapterHealth] {
        try await request(path: "api/v1/adapters", method: "GET")
    }

    public func entityDetail(_ entityID: UUID) async throws -> RobineEntityDetail {
        try await request(path: "api/v1/entities/\(entityID.uuidString)", method: "GET")
    }

    public func createArea(name: String) async throws -> RobineArea {
        try await request(
            path: "api/v1/areas",
            method: "POST",
            body: try JSONEncoder().encode(["name": name])
        )
    }

    public func assign(entityID: UUID, to areaID: UUID?) async throws -> RobineEntity {
        try await request(
            path: "api/v1/entities/\(entityID.uuidString)/area",
            method: "PUT",
            body: try JSONEncoder().encode(["area_id": areaID?.uuidString])
        )
    }

    public func discoverHueBridges() async throws -> [HueBridgeCandidate] {
        try await request(path: "api/v1/adapters/hue/discover", method: "GET")
    }

    public func synchronizeHue() async throws -> HueSynchronization {
        try await request(path: "api/v1/adapters/hue/synchronize", method: "POST")
    }

    public func hueRoomSuggestions() async throws -> [HueRoomSuggestion] {
        try await request(path: "api/v1/adapters/hue/rooms", method: "GET")
    }

    public func importHueRoom(_ suggestion: HueRoomSuggestion) async throws -> RobineArea {
        try await request(
            path: "api/v1/adapters/hue/rooms/import",
            method: "POST",
            body: try JSONEncoder().encode(["suggestion": suggestion])
        )
    }

    public func issueReadOnlyMcpToken(expiresInSeconds: UInt64 = 86_400) async throws -> RobineMcpToken {
        guard (60...2_592_000).contains(expiresInSeconds) else { throw RobineClientError.invalidResponse }
        return try await request(
            path: "api/v1/auth/mcp-tokens",
            method: "POST",
            body: try JSONEncoder().encode([
                "scopes": AnyEncodable(["read"]),
                "expires_in_seconds": AnyEncodable(expiresInSeconds),
            ])
        )
    }

    public func pairHue(
        authority: String,
        certificatePEM: String,
        certificateSHA256: String
    ) async throws -> HuePairingResult {
        try await request(
            path: "api/v1/adapters/hue/pair",
            method: "POST",
            body: try JSONEncoder().encode([
                "authority": authority,
                "certificate_pem": certificatePEM,
                "certificate_sha256": certificateSHA256,
            ])
        )
    }

    /// Le serveur compare cette empreinte à la chaîne PEM qu'il recevra avant
    /// de l'enregistrer comme autorité de confiance du bridge. La saisie
    /// utilisateur ne comporte donc pas de champ d'empreinte sujet aux fautes
    /// de copie.
    public func pairHue(authority: String, certificatePEM: String) async throws -> HuePairingResult {
        let digest = SHA256.hash(data: Data(certificatePEM.utf8))
        let fingerprint = digest.map { String(format: "%02x", $0) }.joined()
        return try await pairHue(
            authority: authority,
            certificatePEM: certificatePEM,
            certificateSHA256: fingerprint
        )
    }

    public func requestCommand(entityID: UUID, key: String, value: Bool, idempotencyKey: UUID = UUID()) async throws -> CommandAccepted {
        let body = try JSONEncoder().encode(["key": AnyEncodable(key), "value": AnyEncodable(value)])
        return try await request(path: "api/v1/entities/\(entityID.uuidString)/commands", method: "POST", body: body, headers: ["Idempotency-Key": idempotencyKey.uuidString])
    }

    public func requestCommand(entityID: UUID, key: String, value: Double, idempotencyKey: UUID = UUID()) async throws -> CommandAccepted {
        let body = try JSONEncoder().encode(["key": AnyEncodable(key), "value": AnyEncodable(value)])
        return try await request(path: "api/v1/entities/\(entityID.uuidString)/commands", method: "POST", body: body, headers: ["Idempotency-Key": idempotencyKey.uuidString])
    }

    public func requestCommand(entityID: UUID, key: String, value: String, idempotencyKey: UUID = UUID()) async throws -> CommandAccepted {
        let body = try JSONEncoder().encode(["key": AnyEncodable(key), "value": AnyEncodable(value)])
        return try await request(path: "api/v1/entities/\(entityID.uuidString)/commands", method: "POST", body: body, headers: ["Idempotency-Key": idempotencyKey.uuidString])
    }

    public func eventStream(after sequence: UInt64?) throws -> URLSessionWebSocketTask {
        var url = server.baseURL.appending(path: "api/v1/stream")
        if let sequence { url.append(queryItems: [URLQueryItem(name: "after", value: String(sequence))]) }
        var request = URLRequest(url: url)
        request.setValue("Bearer \(try tokenProvider())", forHTTPHeaderField: "Authorization")
        return session.webSocketTask(with: request)
    }

    private func request<T: Decodable>(path: String, method: String, body: Data? = nil, headers: [String: String] = [:]) async throws -> T {
        try await request(
            url: server.baseURL.appending(path: path),
            method: method,
            body: body,
            headers: headers
        )
    }

    private func request<T: Decodable>(url: URL, method: String, body: Data? = nil, headers: [String: String] = [:]) async throws -> T {
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.httpBody = body
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        if body != nil { request.setValue("application/json", forHTTPHeaderField: "Content-Type") }
        request.setValue("Bearer \(try tokenProvider())", forHTTPHeaderField: "Authorization")
        for (name, value) in headers { request.setValue(value, forHTTPHeaderField: name) }
        let (data, response) = try await session.data(for: request)
        guard let response = response as? HTTPURLResponse else { throw RobineClientError.invalidResponse }
        guard (200..<300).contains(response.statusCode) else {
            if response.statusCode == 401 { throw RobineClientError.unauthorized }
            if let error = try? JSONDecoder.robine.decode(RobineServerError.self, from: data) {
                throw RobineClientError.server(error.message)
            }
            throw RobineClientError.server(String(decoding: data, as: UTF8.self))
        }
        return try JSONDecoder.robine.decode(T.self, from: data)
    }
}

public enum RobineStreamMessage: Sendable, Equatable {
    case ready(cursor: UInt64)
    case event(id: UInt64, topic: String, eventType: String, data: Data)
    case resyncRequired
}

/// Session WebSocket explicitement souscrite. L’app conserve `cursor` après
/// chaque événement pour reprendre le flux plutôt que de supposer un cache
/// temps réel lors d’un retour au premier plan. Le curseur ne progresse
/// qu'après l'ack, donc après que l'appelant a appliqué la projection locale.
public actor RobineEventStream {
    private let task: URLSessionWebSocketTask
    public private(set) var cursor: UInt64

    public init(client: RobineClient, after: UInt64 = 0) async throws {
        task = try await client.eventStream(after: after)
        cursor = after
    }

    public func connect(topics: [String] = ["state", "device", "area", "adapter", "command", "automation"]) async throws {
        task.resume()
        let body = try JSONSerialization.data(withJSONObject: ["type": "subscribe", "topics": topics, "after": cursor])
        guard let text = String(data: body, encoding: .utf8) else { throw RobineClientError.invalidResponse }
        try await task.send(.string(text))
    }

    public func next() async throws -> RobineStreamMessage {
        let message = try await task.receive()
        let data: Data
        switch message {
        case let .string(text): data = Data(text.utf8)
        case let .data(value): data = value
        @unknown default: throw RobineClientError.invalidResponse
        }
        let object = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        guard let type = object?["type"] as? String else { throw RobineClientError.invalidResponse }
        switch type {
        case "ready":
            guard let cursor = unsignedInteger(object?["cursor"]) else { throw RobineClientError.invalidResponse }
            // `ready` indique la position du serveur, pas un événement déjà
            // appliqué par le client. Le curseur durable ne progresse qu'après
            // application d'un message `event` et son `ack` indicatif.
            return .ready(cursor: cursor)
        case "event":
            guard let id = unsignedInteger(object?["id"]), let topic = object?["topic"] as? String, let eventType = object?["event_type"] as? String else { throw RobineClientError.invalidResponse }
            let eventData = try JSONSerialization.data(withJSONObject: object?["data"] ?? NSNull())
            return .event(id: id, topic: topic, eventType: eventType, data: eventData)
        case "resync_required": return .resyncRequired
        default: throw RobineClientError.invalidResponse
        }
    }

    public func acknowledge(_ id: UInt64) async throws {
        let body = try JSONSerialization.data(withJSONObject: ["type": "ack", "id": id])
        guard let text = String(data: body, encoding: .utf8) else { throw RobineClientError.invalidResponse }
        try await task.send(.string(text))
        cursor = max(cursor, id)
    }

    public func close() { task.cancel(with: .normalClosure, reason: nil) }

    private func unsignedInteger(_ value: Any?) -> UInt64? {
        guard let number = value as? NSNumber, number.int64Value >= 0 else { return nil }
        return number.uint64Value
    }
}

private extension JSONDecoder {
    static let robine: JSONDecoder = {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }()
}

private extension JSONEncoder {
    static let robine: JSONEncoder = {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        encoder.dateEncodingStrategy = .iso8601
        return encoder
    }()
}

private struct AnyEncodable: Encodable {
    let value: Any
    init(_ value: Any) { self.value = value }
    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        if let value = value as? Bool { try container.encode(value) }
        else if let value = value as? String { try container.encode(value) }
        else if let value = value as? Double { try container.encode(value) }
        else { throw RobineClientError.invalidResponse }
    }
}
