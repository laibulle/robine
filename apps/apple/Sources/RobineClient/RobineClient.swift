import Foundation

public struct RobineServer: Sendable, Hashable {
    public let baseURL: URL
    public init(baseURL: URL) { self.baseURL = baseURL }
}

public struct RobineDevice: Codable, Identifiable, Sendable {
    public let id: UUID
    public let name: String
    public let status: String
    public let entities: [RobineEntity]
}

public struct RobineEntity: Codable, Identifiable, Sendable {
    public let id: UUID
    public let name: String
    public let kind: String
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
    private let server: RobineServer
    private let session: URLSession
    private let tokenProvider: @Sendable () throws -> String

    public init(server: RobineServer, session: URLSession = .shared, tokenProvider: @escaping @Sendable () throws -> String) {
        self.server = server
        self.session = session
        self.tokenProvider = tokenProvider
    }

    public func devices() async throws -> [RobineDevice] {
        try await request(path: "api/v1/devices", method: "GET")
    }

    public func requestCommand(entityID: UUID, key: String, value: Bool, idempotencyKey: UUID = UUID()) async throws -> CommandAccepted {
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
        var request = URLRequest(url: server.baseURL.appending(path: path))
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

private extension JSONDecoder {
    static let robine: JSONDecoder = {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }()
}

private struct AnyEncodable: Encodable {
    let value: Any
    init(_ value: Any) { self.value = value }
    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        if let value = value as? Bool { try container.encode(value) }
        else if let value = value as? String { try container.encode(value) }
        else { throw RobineClientError.invalidResponse }
    }
}
