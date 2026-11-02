import Foundation

struct AgentError: Error, LocalizedError, Sendable {
    var code: String
    var message: String
    var errorDescription: String? { message }

    static let notRunning = AgentError(code: "not-running", message: "The Homebase agent isn't running.")
    static func io(_ what: String) -> AgentError {
        AgentError(code: "io", message: "\(what): \(String(cString: strerror(errno)))")
    }
}

/// Talks to hbd over its Unix socket: one JSON request line, one JSON response line.
struct AgentClient: Sendable {
    let socketPath: String

    static func defaultSocketPath() -> String {
        let env = ProcessInfo.processInfo.environment
        if let dir = env["HOMEBASE_DATA_DIR"], !dir.isEmpty {
            return (dir as NSString).appendingPathComponent("hbd.sock")
        }
        let support = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/Homebase", isDirectory: true)
        return support.appendingPathComponent("hbd.sock").path
    }

    func call<T: Decodable & Sendable>(_ request: AgentRequest, as type: T.Type = T.self) async throws -> T {
        try await Task.detached(priority: .userInitiated) {
            try self.callSync(request, as: type)
        }.value
    }

    func callSync<T: Decodable>(_ request: AgentRequest, as type: T.Type) throws -> T {
        let body = try JSONEncoder().encode(request)
        let line = try roundTrip(body)
        guard let object = try JSONSerialization.jsonObject(with: line) as? [String: Any] else {
            throw AgentError(code: "io", message: "The agent sent something that isn't JSON.")
        }
        guard object["ok"] as? Bool == true else {
            let error = object["error"] as? [String: Any]
            throw AgentError(
                code: error?["code"] as? String ?? "failed",
                message: error?["message"] as? String ?? "The agent returned an error."
            )
        }
        let data = try JSONSerialization.data(
            withJSONObject: object["data"] ?? NSNull(), options: [.fragmentsAllowed])
        return try JSONDecoder().decode(T.self, from: data)
    }

    private func roundTrip(_ body: Data) throws -> Data {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw AgentError.io("socket") }
        defer { close(fd) }

        var on: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
        var timeout = timeval(tv_sec: 30, tv_usec: 0)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))

        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let path = Array(socketPath.utf8)
        guard path.count < MemoryLayout.size(ofValue: address.sun_path) else {
            throw AgentError(code: "io", message: "The socket path is too long: \(socketPath)")
        }
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            raw.copyBytes(from: path)
            raw[path.count] = 0
        }
        let connected = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard connected == 0 else { throw AgentError.notRunning }

        var payload = body
        payload.append(0x0A)
        try payload.withUnsafeBytes { (buffer: UnsafeRawBufferPointer) in
            var offset = 0
            while offset < buffer.count {
                let n = write(fd, buffer.baseAddress! + offset, buffer.count - offset)
                guard n > 0 else { throw AgentError.io("write") }
                offset += n
            }
        }

        var response = Data()
        var chunk = [UInt8](repeating: 0, count: 64 * 1024)
        while true {
            let n = read(fd, &chunk, chunk.count)
            guard n > 0 else { break }
            response.append(contentsOf: chunk[0..<n])
            if let newline = response.firstIndex(of: 0x0A) {
                return response.prefix(upTo: newline)
            }
        }
        throw AgentError(code: "io", message: "The agent closed the connection without answering.")
    }
}
