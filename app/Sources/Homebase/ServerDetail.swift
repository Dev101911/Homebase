import SwiftUI

/// The form for one server. Every field maps to one claude remote-control flag.
struct ServerDetail: View {
    @Environment(Store.self) private var store
    @Environment(\.openWindow) private var openWindow
    let saved: Server
    @State private var draft: Server
    @State private var confirmingDelete = false
    @State private var saving = false

    init(saved: Server) {
        self.saved = saved
        _draft = State(initialValue: saved)
    }

    private var status: ServerStatus? { store.status(for: saved.id) }
    private var isDirty: Bool { draft != saved }

    var body: some View {
        Form {
            Section {
                TextField("Name", text: $draft.name, prompt: Text("Shown in the Claude app"))
                LabeledContent("Folder") {
                    HStack {
                        Text(draft.folder.abbreviatingHome)
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .textSelection(.enabled)
                        Button("Choose…") {
                            if let url = Tools.chooseFolder(startingAt: draft.folderURL) {
                                draft.folder = url.path
                            }
                        }
                    }
                }
                LabeledContent("ID") {
                    Text(draft.id).monospaced().textSelection(.enabled)
                }
            } header: {
                statusHeader
            }

            Section("Remote Control") {
                Picker("Spawn mode", selection: $draft.spawn) {
                    Text("Automatic").tag("auto")
                    Text("Same directory").tag("same-dir")
                    Text("Worktree").tag("worktree")
                    Text("Single session").tag("session")
                }
                OptionalBoolPicker(
                    title: "Create a session on start", value: $draft.createSessionInDir)
                OptionalIntField(title: "Capacity", value: $draft.capacity, placeholder: "claude's default (32)")
                Picker("Permission mode", selection: $draft.permissionMode) {
                    Text("claude's default").tag(String?.none)
                    ForEach(["default", "acceptEdits", "auto", "plan", "dontAsk", "bypassPermissions"], id: \.self) {
                        Text($0).tag(String?.some($0))
                    }
                }
                OptionalBoolPicker(title: "Claude in Chrome", value: $draft.chrome)
                OptionalTextField(title: "Session name prefix", value: $draft.namePrefix, placeholder: "claude's default (hostname)")
                TextField("Extra arguments", text: extraArgs, prompt: Text("Passed as-is, separated by spaces"))
            }

            Section {
                Toggle("Start with “Start All” and at login", isOn: $draft.autoStart)
                Picker("If it exits", selection: $draft.restart) {
                    Text("Restart if it fails").tag("on-failure")
                    Text("Always restart").tag("always")
                    Text("Leave it stopped").tag("never")
                }
            } header: {
                Text("Homebase")
            } footer: {
                Text("Automatic restarts arrive with the reliability milestone. Until then a server that exits stays stopped.")
                    .foregroundStyle(.secondary)
            }

            Section("Command that runs") {
                Text(status?.command ?? "…")
                    .font(.system(.callout, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if isDirty {
                    Text("Save to see the command with your changes.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            if let status, !status.lastLines.isEmpty {
                Section("Last output") {
                    Text(status.lastLines.suffix(8).joined(separator: "\n"))
                        .font(.system(.caption, design: .monospaced))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
        .formStyle(.grouped)
        .navigationTitle(saved.name)
        .toolbar { toolbar }
        .safeAreaInset(edge: .bottom) { bottomBar }
        .confirmationDialog(
            "Delete “\(saved.name)”?", isPresented: $confirmingDelete, titleVisibility: .visible
        ) {
            Button("Delete Server", role: .destructive) {
                Task { await store.remove(saved.id) }
            }
        } message: {
            Text("The server stops and is removed from Homebase. The folder \(saved.folder.abbreviatingHome) and its worktrees are left alone.")
        }
        .onChange(of: saved) { old, new in
            // Pick up changes made elsewhere (hb, a hand edit) unless there are unsaved edits.
            if draft == old { draft = new }
        }
    }

    private var statusHeader: some View {
        HStack(spacing: 8) {
            StateDot(state: status?.state)
            Text(status?.state.label ?? "Stopped")
            if let status, status.state.isLive, let started = status.startedDate {
                Text("· up \(Date().timeIntervalSince(started).uptimeText)")
            }
            if let pid = status?.pid {
                Text("· pid \(pid)").monospacedDigit()
            }
            if let restarts = status?.restarts, restarts > 0 {
                Text("· \(restarts) restarts")
            }
            if let code = status?.lastExit, status?.state.isLive == false {
                Text("· last exit \(code)")
            }
        }
        .font(.callout)
        .foregroundStyle(.secondary)
        .textCase(nil)
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItemGroup {
            if store.busy.contains(saved.id) {
                ProgressView().controlSize(.small)
            }
            if status?.state.isLive == true {
                Button { Task { await store.stop(saved.id) } } label: {
                    Label("Stop", systemImage: "stop.fill")
                }
                Button { Task { await store.restart(saved.id) } } label: {
                    Label("Restart", systemImage: "arrow.clockwise")
                }
            } else {
                Button { Task { await store.start(saved.id) } } label: {
                    Label("Start", systemImage: "play.fill")
                }
            }
            Button {
                store.logServerID = saved.id
                openWindow(id: "log")
            } label: {
                Label("Log", systemImage: "doc.text.magnifyingglass")
            }
            .help("Show this server's log")
        }
    }

    private var bottomBar: some View {
        HStack {
            Button("Delete…", role: .destructive) { confirmingDelete = true }
            if status?.state == .needsAttention {
                Button("Open Terminal Here") { Tools.openTerminal(at: saved.folderURL) }
                    .help("Run claude, answer its question, exit, then Restart.")
            }
            Spacer()
            Button("Revert") { draft = saved }
                .disabled(!isDirty)
            Button(status?.state.isLive == true ? "Save and Restart" : "Save") {
                Task {
                    saving = true
                    if let new = await store.save(draft) { draft = new }
                    saving = false
                }
            }
            .keyboardShortcut("s", modifiers: .command)
            .buttonStyle(.borderedProminent)
            .disabled(!isDirty || saving || draft.name.trimmingCharacters(in: .whitespaces).isEmpty)
        }
        .padding(12)
        .background(.bar)
    }

    private var extraArgs: Binding<String> {
        Binding(
            get: { draft.extraArgs.joined(separator: " ") },
            set: { draft.extraArgs = $0.split(whereSeparator: \.isWhitespace).map(String.init) })
    }
}

/// claude's default / On / Off, for a flag with a --no- form.
struct OptionalBoolPicker: View {
    var title: String
    @Binding var value: Bool?

    var body: some View {
        Picker(title, selection: $value) {
            Text("claude's default").tag(Bool?.none)
            Text("On").tag(Bool?.some(true))
            Text("Off").tag(Bool?.some(false))
        }
    }
}

struct OptionalIntField: View {
    var title: String
    @Binding var value: Int?
    var placeholder: String

    var body: some View {
        TextField(title, text: Binding(
            get: { value.map(String.init) ?? "" },
            set: { text in
                let digits = text.filter(\.isNumber)
                value = digits.isEmpty ? nil : Int(digits).map { max(1, $0) }
            }), prompt: Text(placeholder))
    }
}

struct OptionalTextField: View {
    var title: String
    @Binding var value: String?
    var placeholder: String

    var body: some View {
        TextField(title, text: Binding(
            get: { value ?? "" },
            set: { value = $0.isEmpty ? nil : $0 }), prompt: Text(placeholder))
    }
}
