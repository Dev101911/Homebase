import SwiftUI

/// Follows a server's log file. The agent already strips terminal colours.
struct LogWindow: View {
    @Environment(Store.self) private var store
    @State private var lines: [String] = []
    @State private var follow = true
    @State private var lastSize: UInt64 = 0

    private static let maxLines = 3000

    var body: some View {
        @Bindable var store = store
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    Text(lines.isEmpty ? "No output yet." : lines.joined(separator: "\n"))
                        .font(.system(.callout, design: .monospaced))
                        .foregroundStyle(lines.isEmpty ? .secondary : .primary)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(12)
                    Color.clear.frame(height: 1).id("bottom")
                }
                .onChange(of: lines) {
                    if follow { proxy.scrollTo("bottom", anchor: .bottom) }
                }
            }
            Divider()
            HStack {
                Picker("Server", selection: $store.logServerID) {
                    ForEach(store.servers) { Text($0.name).tag(String?.some($0.id)) }
                }
                .frame(maxWidth: 240)
                Toggle("Follow", isOn: $follow)
                Spacer()
                Button("Copy") { Tools.copy(lines.joined(separator: "\n")) }
                Button("Reveal in Finder") { if let path = logPath { Tools.revealInFinder(path) } }
                    .disabled(logPath == nil)
            }
            .padding(10)
        }
        .navigationTitle(title)
        .task(id: store.logServerID) { await poll() }
        .onAppear {
            DockIcon.windowOpened()
            if store.logServerID == nil { store.logServerID = store.servers.first?.id }
        }
        .onDisappear { DockIcon.windowClosed() }
    }

    private var title: String {
        guard let id = store.logServerID else { return "Server Log" }
        return "\(store.server(for: id)?.name ?? id) — Log"
    }

    private var logPath: String? {
        guard let id = store.logServerID else { return nil }
        return store.status(for: id)?.logFile
    }

    private func poll() async {
        lines = []
        lastSize = 0
        while !Task.isCancelled {
            if let path = logPath {
                let size = (try? FileManager.default.attributesOfItem(atPath: path)[.size] as? UInt64) ?? 0
                if size != lastSize {
                    lastSize = size
                    lines = await Self.tail(path: path, maxLines: Self.maxLines)
                }
            }
            try? await Task.sleep(for: .seconds(1))
        }
    }

    /// The last lines of a file, reading at most the final 1 MB.
    private static func tail(path: String, maxLines: Int) async -> [String] {
        await Task.detached {
            guard let handle = FileHandle(forReadingAtPath: path) else { return [] }
            defer { try? handle.close() }
            let end = (try? handle.seekToEnd()) ?? 0
            let start = end > 1_000_000 ? end - 1_000_000 : 0
            try? handle.seek(toOffset: start)
            let data = (try? handle.readToEnd()) ?? Data()
            var all = String(decoding: data, as: UTF8.self).components(separatedBy: "\n")
            if start > 0, !all.isEmpty { all.removeFirst() }  // likely a partial line
            if all.last == "" { all.removeLast() }
            return Array(all.suffix(maxLines))
        }.value
    }
}
