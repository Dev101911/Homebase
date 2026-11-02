import Foundation

/// One `claude remote-control` server, as stored in servers.json.
/// `nil` means the flag isn't passed, so claude's own default applies.
struct Server: Codable, Hashable, Identifiable, Sendable {
    var id: String
    var name: String
    var folder: String
    var spawn: String = "auto"
    var createSessionInDir: Bool? = false
    var capacity: Int?
    var permissionMode: String?
    var chrome: Bool?
    var namePrefix: String?
    var extraArgs: [String] = []
    var env: [String: String] = [:]
    var autoStart: Bool = true
    var restart: String = "on-failure"

    var folderURL: URL {
        URL(fileURLWithPath: (folder as NSString).expandingTildeInPath, isDirectory: true)
    }
}

enum ServerState: String, Codable, Sendable {
    case stopped
    case starting
    case running
    case needsAttention = "needs-attention"
    case failed
    case backingOff = "backing-off"

    var label: String {
        switch self {
        case .stopped: "Stopped"
        case .starting: "Starting"
        case .running: "Running"
        case .needsAttention: "Needs attention"
        case .failed: "Failed"
        case .backingOff: "Waiting to restart"
        }
    }

    /// A process exists for the server.
    var isLive: Bool { self == .starting || self == .running || self == .needsAttention }
}

struct ServerStatus: Codable, Hashable, Identifiable, Sendable {
    var id: String
    var name: String
    var folder: String
    var state: ServerState
    var pid: Int?
    var startedAt: Int?
    var restarts: Int
    var lastExit: Int?
    var attention: String?
    var command: String?
    var lastLines: [String]
    var logFile: String

    var startedDate: Date? { startedAt.map { Date(timeIntervalSince1970: TimeInterval($0)) } }

    /// The trust question rather than sign-in or a crash.
    var needsTrust: Bool { attention?.localizedCaseInsensitiveContains("trust") ?? false }
}

struct ActionResult: Codable, Sendable {
    var id: String
    var ok: Bool
    var message: String?
}

struct AgentConfig: Codable, Sendable {
    var version: Int
    var claudePath: String?
    var servers: [Server]
    /// Set when servers.json on disk is invalid and the agent kept the last good copy.
    var error: String?
}

struct Pong: Codable, Sendable {
    var version: String
    var `protocol`: Int
    var pid: Int
}

/// One line on the socket. Nil fields are left out.
struct AgentRequest: Encodable, Sendable {
    var cmd: String
    var id: String?
    var ids: [String]?
    var all: Bool?
    var server: Server?
    var restart: Bool?

    static let ping = AgentRequest(cmd: "ping")
    static let list = AgentRequest(cmd: "list")
    static let getConfig = AgentRequest(cmd: "get-config")
    static func status(_ id: String) -> AgentRequest { AgentRequest(cmd: "status", id: id) }
    static func start(_ ids: [String]) -> AgentRequest { AgentRequest(cmd: "start", ids: ids) }
    static let startAll = AgentRequest(cmd: "start", all: true)
    static func stop(_ ids: [String]) -> AgentRequest { AgentRequest(cmd: "stop", ids: ids) }
    static let stopAll = AgentRequest(cmd: "stop", all: true)
    static func restart(_ ids: [String]) -> AgentRequest { AgentRequest(cmd: "restart", ids: ids) }
    static func add(_ server: Server) -> AgentRequest { AgentRequest(cmd: "add", server: server) }
    static func edit(_ server: Server, restart: Bool) -> AgentRequest {
        AgentRequest(cmd: "edit", server: server, restart: restart)
    }
    static func remove(_ id: String) -> AgentRequest { AgentRequest(cmd: "remove", id: id) }
}

enum Slug {
    /// "My App" -> "my-app". Matches homebase-core's slugify.
    static func make(_ name: String) -> String {
        var out = ""
        for scalar in name.unicodeScalars {
            if scalar.isASCII, CharacterSet.alphanumerics.contains(scalar) {
                out.append(Character(scalar).lowercased())
            } else if !out.isEmpty, !out.hasSuffix("-") {
                out.append("-")
            }
        }
        while out.hasSuffix("-") { out.removeLast() }
        out = String(out.prefix(64))
        while out.hasSuffix("-") { out.removeLast() }
        return out.isEmpty ? "server" : out
    }

    static func isValid(_ id: String) -> Bool {
        guard let first = id.first, first.isLowercase || first.isNumber, id.count <= 64 else { return false }
        return id.allSatisfy { ($0.isASCII && ($0.isLowercase || $0.isNumber)) || $0 == "-" }
    }
}

extension TimeInterval {
    /// "45s", "12m", "3h 12m", "2d 4h".
    var uptimeText: String {
        let s = max(0, Int(self))
        switch s {
        case ..<60: return "\(s)s"
        case ..<3600: return "\(s / 60)m"
        case ..<86_400: return "\(s / 3600)h \(s % 3600 / 60)m"
        default: return "\(s / 86_400)d \(s % 86_400 / 3600)h"
        }
    }
}

extension String {
    /// `$HOME/x` -> `~/x` for display.
    var abbreviatingHome: String { (self as NSString).abbreviatingWithTildeInPath }
}
