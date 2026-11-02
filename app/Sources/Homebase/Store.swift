import AppKit
import Observation
import SwiftUI

/// Everything the windows show, refreshed from the agent every two seconds.
/// The app holds no supervision logic: every change goes through hbd.
@MainActor
@Observable
final class Store {
    enum Overall {
        case agentDown, idle, allRunning, someStopped, attention, failed
    }

    private(set) var statuses: [ServerStatus] = []
    private(set) var servers: [Server] = []
    private(set) var configError: String?
    private(set) var agentRunning = false
    private(set) var agentInfo: Pong?
    private(set) var lastRefresh = Date()
    /// Server ids with a request in flight, so buttons can show progress.
    private(set) var busy: Set<String> = []
    /// The last failed action, shown as a banner until dismissed.
    var actionError: String?

    /// The sidebar selection in Settings: a server id or `Store.generalTab`.
    var selection: String?
    var showingAddSheet = false
    /// Which server's log the log window follows.
    var logServerID: String?

    static let generalTab = "::general"

    let client = AgentClient(socketPath: AgentClient.defaultSocketPath())
    private var lastAgentLaunch = Date.distantPast
    private var pollTask: Task<Void, Never>?

    init() {
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                await self?.refresh()
                try? await Task.sleep(for: .seconds(2))
            }
        }
    }

    // MARK: Reading

    func status(for id: String) -> ServerStatus? { statuses.first { $0.id == id } }
    func server(for id: String) -> Server? { servers.first { $0.id == id } }

    var overall: Overall {
        guard agentRunning else { return .agentDown }
        if statuses.contains(where: { $0.state == .failed }) { return .failed }
        if statuses.contains(where: { $0.state == .needsAttention }) { return .attention }
        let live = statuses.filter { $0.state.isLive || $0.state == .backingOff }
        if live.isEmpty { return .idle }
        return live.count == statuses.count ? .allRunning : .someStopped
    }

    func refresh() async {
        do {
            async let list: [ServerStatus] = client.call(.list)
            async let config: AgentConfig = client.call(.getConfig)
            let (l, c) = try await (list, config)
            statuses = l
            servers = c.servers
            configError = c.error
            if !agentRunning || agentInfo == nil {
                agentInfo = try? await client.call(.ping)
            }
            agentRunning = true
        } catch let error as AgentError where error.code == "not-running" {
            agentRunning = false
            agentInfo = nil
            await launchAgentIfDue()
        } catch {
            agentRunning = false
        }
        lastRefresh = Date()
    }

    /// Starts hbd through the bundled `hb`, which launches it detached, so
    /// quitting the app leaves the servers running. At most every 10 seconds.
    private func launchAgentIfDue() async {
        guard Date().timeIntervalSince(lastAgentLaunch) > 10 else { return }
        lastAgentLaunch = Date()
        _ = await Tools.runHB(["agent", "status"])  // cheap check that hb exists
        _ = await Tools.runHB(["ls", "--json"])      // starts the agent when needed
    }

    func retryAgent() async {
        lastAgentLaunch = .distantPast
        await refresh()
    }

    // MARK: Actions

    func start(_ id: String) async { await act([id]) { try await self.client.call(.start([id])) } }
    func stop(_ id: String) async { await act([id]) { try await self.client.call(.stop([id])) } }
    func restart(_ id: String) async { await act([id]) { try await self.client.call(.restart([id])) } }
    func startAll() async { await act(servers.map(\.id)) { try await self.client.call(.startAll) } }
    func stopAll() async { await act(servers.map(\.id)) { try await self.client.call(.stopAll) } }

    private func act(_ ids: [String], _ body: @escaping () async throws -> [ActionResult]) async {
        busy.formUnion(ids)
        defer { busy.subtract(ids) }
        do {
            let results = try await body()
            let failures = results.filter { !$0.ok }
            if !failures.isEmpty {
                actionError = failures.map { "\($0.id): \($0.message ?? "failed")" }.joined(separator: "\n")
            }
        } catch {
            actionError = error.localizedDescription
        }
        await refresh()
    }

    /// Adds a server, optionally starting it. Returns the saved server, or nil and sets `actionError`.
    func add(_ server: Server, start: Bool) async -> Server? {
        do {
            let saved: Server = try await client.call(.add(server))
            if start {
                let _: [ActionResult] = try await client.call(.start([saved.id]))
            }
            await refresh()
            selection = saved.id
            return saved
        } catch {
            actionError = error.localizedDescription
            return nil
        }
    }

    func save(_ server: Server, restartIfRunning: Bool = true) async -> Server? {
        busy.insert(server.id)
        defer { busy.remove(server.id) }
        do {
            let saved: Server = try await client.call(.edit(server, restart: restartIfRunning))
            await refresh()
            return saved
        } catch {
            actionError = error.localizedDescription
            return nil
        }
    }

    func remove(_ id: String) async {
        busy.insert(id)
        defer { busy.remove(id) }
        do {
            let _: Server = try await client.call(.remove(id))
            if selection == id { selection = nil }
        } catch {
            actionError = error.localizedDescription
        }
        await refresh()
    }
}
