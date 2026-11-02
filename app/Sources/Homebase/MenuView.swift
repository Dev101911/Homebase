import SwiftUI

/// The popover: a glance at every server, with the one obvious action each.
struct MenuView: View {
    @Environment(Store.self) private var store
    @Environment(\.openSettings) private var openSettings
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            Divider()
            content
            if let error = store.actionError {
                Divider()
                ErrorBanner(message: error) { store.actionError = nil }
                    .padding(10)
            }
            Divider()
            footer
        }
        .frame(width: 360)
    }

    private var header: some View {
        HStack {
            Text("Homebase").font(.headline)
            Spacer()
            Text(summary)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    private var summary: String {
        guard store.agentRunning else { return "Agent not running" }
        let running = store.statuses.filter { $0.state.isLive }.count
        return "\(running) of \(store.statuses.count) running"
    }

    @ViewBuilder
    private var content: some View {
        if !store.agentRunning {
            VStack(alignment: .leading, spacing: 8) {
                Text("The Homebase agent isn't running.")
                Text("It starts automatically. If it doesn't, run `hb doctor` in Terminal.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Button("Try Again") { Task { await store.retryAgent() } }
            }
            .padding(14)
        } else if store.statuses.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                Text("No servers yet.")
                Text("Add a project folder to make it reachable from the Claude app.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Button("Add Server…") { showSettings(addServer: true) }
            }
            .padding(14)
        } else {
            ScrollView {
                VStack(spacing: 0) {
                    ForEach(store.statuses) { status in
                        ServerRow(status: status) { showSettings(select: status.id) }
                        if status.id != store.statuses.last?.id {
                            Divider().padding(.leading, 30)
                        }
                    }
                }
            }
            .frame(maxHeight: 420)
            .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var footer: some View {
        HStack(spacing: 12) {
            Button {
                showSettings(addServer: true)
            } label: {
                Label("Add Server", systemImage: "plus")
            }
            .disabled(!store.agentRunning)

            Menu {
                Button("Start All") { Task { await store.startAll() } }
                Button("Stop All") { Task { await store.stopAll() } }
            } label: {
                Label("All", systemImage: "play.square.stack")
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .disabled(store.statuses.isEmpty)

            Spacer()

            Button {
                showSettings()
            } label: {
                Image(systemName: "gearshape")
            }
            .help("Settings")

            Button {
                NSApp.terminate(nil)
            } label: {
                Image(systemName: "power")
            }
            .help("Quit Homebase. Your servers keep running.")
        }
        .buttonStyle(.borderless)
        .labelStyle(.titleAndIcon)
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    private func showSettings(select id: String? = nil, addServer: Bool = false) {
        if let id { store.selection = id }
        if addServer { store.showingAddSheet = true }
        NSApp.activate()
        openSettings()
    }
}

struct ServerRow: View {
    @Environment(Store.self) private var store
    var status: ServerStatus
    var open: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            StateDot(state: status.state)
                .padding(.top, 5)
            VStack(alignment: .leading, spacing: 2) {
                Text(status.name).fontWeight(.medium)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                if let attention = status.attention, status.state != .stopped {
                    Text(attention)
                        .font(.caption)
                        .foregroundStyle(status.state == .failed ? .red : .orange)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 8)
            action
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .contentShape(Rectangle())
        .onTapGesture(perform: open)
        .contextMenu {
            Button("Start") { Task { await store.start(status.id) } }.disabled(status.state.isLive)
            Button("Stop") { Task { await store.stop(status.id) } }.disabled(!status.state.isLive)
            Button("Restart") { Task { await store.restart(status.id) } }
            Divider()
            Button("Open in Settings", action: open)
            Button("Show in Finder") { NSWorkspace.shared.open(URL(fileURLWithPath: (status.folder as NSString).expandingTildeInPath)) }
        }
    }

    private var subtitle: String {
        var parts = [status.state.label]
        if status.state.isLive, let started = status.startedDate {
            parts.append(Date().timeIntervalSince(started).uptimeText)
        }
        parts.append(status.folder.abbreviatingHome)
        return parts.joined(separator: " · ")
    }

    @ViewBuilder
    private var action: some View {
        if store.busy.contains(status.id) {
            ProgressView().controlSize(.small)
        } else {
            switch status.state {
            case .needsAttention:
                Button("Fix") {
                    Tools.openTerminal(at: URL(fileURLWithPath: (status.folder as NSString).expandingTildeInPath))
                }
                .help(status.needsTrust
                    ? "Opens Terminal here. Run claude, accept the trust question, exit, then Restart."
                    : "Opens Terminal. Run claude auth login, then Restart.")
            case .running, .starting:
                Button("Stop") { Task { await store.stop(status.id) } }
            case .stopped, .failed, .backingOff:
                Button("Start") { Task { await store.start(status.id) } }
            }
        }
    }
}

struct ErrorBanner: View {
    var message: String
    var dismiss: () -> Void

    var body: some View {
        HStack(alignment: .top) {
            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
            Text(message)
                .font(.caption)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
            Spacer()
            Button(action: dismiss) { Image(systemName: "xmark") }
                .buttonStyle(.borderless)
                .help("Dismiss")
        }
    }
}
