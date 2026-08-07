import CryptoKit
import Foundation
import XCTest
@testable import RobineClient

final class HueBridgeCertificateTests: XCTestCase {
    func testCertificateUsesPemBytesForServerCompatibleFingerprint() {
        let certificate = HueBridgeCertificate.fromDER(Data([0x01, 0x02, 0x03]))

        XCTAssertEqual(
            certificate.pem,
            "-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n"
        )
        let expected = SHA256.hash(data: Data(certificate.pem.utf8))
            .map { String(format: "%02x", $0) }
            .joined()
        XCTAssertEqual(certificate.sha256, expected)
        XCTAssertEqual(certificate.shortFingerprint, "1ce4:f4d4:0b3f:a446")
    }

    func testServerCertificatePinsTheLeafDerRatherThanPresentationPem() {
        let der = Data([0x01, 0x02, 0x03])
        let certificate = RobineServerCertificate.fromDER(der)
        let expected = SHA256.hash(data: der)
            .map { String(format: "%02x", $0) }
            .joined()
        XCTAssertEqual(certificate.sha256, expected)
        XCTAssertEqual(certificate.shortFingerprint.count, 19)
    }

    func testSimulationDecodesTheRustRunTraceWithoutExecutingIt() throws {
        let payload = """
        {
          "name": "Soir paisible",
          "command_count": 0,
          "diagnostics": [],
          "result": {
            "status": "completed",
            "steps": [{ "type": "audit", "message": "ok" }]
          }
        }
        """

        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let simulation = try decoder.decode(RobineFlowSimulation.self, from: Data(payload.utf8))
        XCTAssertEqual(simulation.name, "Soir paisible")
        XCTAssertEqual(simulation.commandCount, 0)
        XCTAssertEqual(simulation.result.status, "completed")
        XCTAssertEqual(simulation.result.steps.map { $0.description }, ["ok"])
    }

    func testSimulationExplainsPersistedRetrySteps() throws {
        let payload = """
        {
          "status": "failed",
          "steps": [
            { "type": "retry_scheduled", "next_attempt": 2, "total_attempts": 3, "backoff_milliseconds": 200 },
            { "type": "retry_exhausted", "attempts": 3 }
          ]
        }
        """
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let result = try decoder.decode(RobineFlowSimulationResult.self, from: Data(payload.utf8))
        XCTAssertEqual(result.status, "failed")
        XCTAssertEqual(result.steps.map(\.description), [
            "Nouvel essai 2/3 programmé après 200 ms.",
            "Les 3 tentatives ont échoué."
        ])
    }

    func testAutomationRunHistoryDecodesThePersistedRuntimeResult() throws {
        let payload = """
        [{
          "id": "00000000-0000-0000-0000-000000000010",
          "flow_id": "00000000-0000-0000-0000-000000000020",
          "recorded_at": "2026-08-07T12:00:00Z",
          "result": { "status": "completed", "steps": [{ "type": "audit", "message": "done" }] }
        }]
        """
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        let runs = try decoder.decode([RobineAutomationRun].self, from: Data(payload.utf8))
        XCTAssertEqual(runs.count, 1)
        XCTAssertEqual(runs[0].flowID.uuidString, "00000000-0000-0000-0000-000000000020".uppercased())
        XCTAssertEqual(runs[0].result.steps.map(\.description), ["done"])
    }

    func testVerifiedBackupManifestDecodesWithoutAnySecret() throws {
        let payload = """
        {
          "manifest_version": 1,
          "created_at": "2026-08-07T12:00:00Z",
          "database_file": "robine-20260807.sqlite3",
          "bytes": 42,
          "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }
        """
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let backup = try decoder.decode(RobineBackup.self, from: Data(payload.utf8))
        XCTAssertEqual(backup.manifestVersion, 1)
        XCTAssertEqual(backup.databaseFile, "robine-20260807.sqlite3")
        XCTAssertEqual(backup.bytes, 42)
        XCTAssertEqual(backup.sha256, String(repeating: "a", count: 64))
    }

    func testDiagnosticsContractsDecodeTheHealthAndHueSynchronization() throws {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let health = try decoder.decode(
            RobineServerHealth.self,
            from: Data("{\"status\":\"degraded\",\"initialized\":true,\"degraded_adapters\":[\"hue:bridge-a\"]}".utf8)
        )
        let synchronization = try decoder.decode(
            HueSynchronization.self,
            from: Data("{\"discovered_devices\":2}".utf8)
        )
        XCTAssertEqual(health.status, "degraded")
        XCTAssertEqual(health.degradedAdapters, ["hue:bridge-a"])
        XCTAssertEqual(synchronization.discoveredDevices, 2)

        let adapter = try decoder.decode(
            RobineAdapterHealth.self,
            from: Data("{\"adapter_id\":\"hue:bridge-a\",\"status\":\"degraded\",\"detail\":\"bridge unavailable\",\"observed_at\":\"2026-08-07T12:00:00Z\"}".utf8)
        )
        XCTAssertEqual(adapter.id, "hue:bridge-a")
        XCTAssertEqual(adapter.detail, "bridge unavailable")

        let mcpToken = try decoder.decode(
            RobineMcpToken.self,
            from: Data("{\"token\":\"shown-once\",\"token_id\":\"mcp_123\",\"expires_at\":\"2026-08-08T12:00:00Z\",\"scopes\":[\"robine:read\"]}".utf8)
        )
        XCTAssertEqual(mcpToken.token, "shown-once")
        XCTAssertEqual(mcpToken.scopes, ["robine:read"])
    }
}
