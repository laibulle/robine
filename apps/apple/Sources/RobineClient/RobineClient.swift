import Foundation
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

    public init(cursor: UInt64, devices: [RobineDevice], areas: [RobineArea], states: [RobineStateProperty]) {
        self.cursor = cursor
        self.devices = devices
        self.areas = areas
        self.states = states
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

public struct HueBridgeCandidate: Codable, Identifiable, Sendable {
    public let name: String
    public let host: String
    public let addresses: [String]
    public var id: String { host }
}

public struct HuePairingResult: Codable, Sendable {
    public let adapterId: String
    public let discoveredDevices: Int
}

public struct CommandAccepted: Codable, Sendable {
    public let commandId: UUID
    public let correlationId: String
}

public enum RobineClientError: LocalizedError {
    case missingToken, invalidResponse, unauthorized, server(String)
    public var errorDescription: String? {
        switch self {
        case .missingToken: "La connexion à Robine doit être associée à un accès local."
        case .invalidResponse: "La réponse de Robine est invalide."
        case .unauthorized: "L’accès local Robine a expiré ou n’est plus autorisé."
        case let .server(message): message
        }
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

    public func recentEvents(limit: UInt = 20) async throws -> RobineEventPage {
        guard (1...500).contains(limit) else { throw RobineClientError.invalidResponse }
        var components = URLComponents(url: server.baseURL.appending(path: "api/v1/events"), resolvingAgainstBaseURL: false)
        components?.queryItems = [URLQueryItem(name: "tail", value: String(limit))]
        guard let url = components?.url else { throw RobineClientError.invalidResponse }
        return try await request(url: url, method: "GET")
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
/// temps réel lors d’un retour au premier plan.
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
            self.cursor = max(cursor, id)
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
