import SwiftUI

/// Pick a folder; the name and id follow from it.
struct AddServerSheet: View {
    @Environment(Store.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var folder: URL?
    @State private var name = ""
    @State private var id = ""
    @State private var idEdited = false
    @State private var spawn = "auto"
    @State private var autoStart = true
    @State private var startNow = true
    @State private var adding = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Form {
                LabeledContent("Folder") {
                    HStack {
                        Text(folder?.path.abbreviatingHome ?? "None chosen")
                            .foregroundStyle(folder == nil ? .secondary : .primary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Button("Choose…", action: choose)
                    }
                }
                TextField("Name", text: $name, prompt: Text("Shown in the Claude app"))
                    .onChange(of: name) { _, new in
                        if !idEdited { id = Slug.make(new) }
                    }
                TextField("ID", text: Binding(
                    get: { id },
                    set: { id = $0; idEdited = true }), prompt: Text("for hb commands"))
                    .monospaced()
                Picker("Spawn mode", selection: $spawn) {
                    Text("Automatic").tag("auto")
                    Text("Same directory").tag("same-dir")
                    Text("Worktree").tag("worktree")
                    Text("Single session").tag("session")
                }
                Toggle("Start with “Start All” and at login", isOn: $autoStart)
                Toggle("Start it now", isOn: $startNow)
                if let problem {
                    Text(problem).font(.caption).foregroundStyle(.orange)
                }
            }
            .formStyle(.grouped)

            Text("The first time claude runs in a folder it asks whether you trust it. If the server shows Needs attention, use Open Terminal Here, run claude once, accept, then Restart.")
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, 20)

            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Add Server") { Task { await add() } }
                    .keyboardShortcut(.defaultAction)
                    .disabled(problem != nil || adding)
            }
            .padding(20)
        }
        .frame(width: 480)
        .onAppear { if folder == nil { choose() } }
    }

    private var problem: String? {
        if folder == nil { return "Choose a project folder." }
        if name.trimmingCharacters(in: .whitespaces).isEmpty { return "Give the server a name." }
        if !Slug.isValid(id) { return "The ID must be lower-case letters, digits and dashes." }
        if store.server(for: id) != nil { return "A server with the ID “\(id)” already exists." }
        return nil
    }

    private func choose() {
        guard let url = Tools.chooseFolder(startingAt: folder) else { return }
        folder = url
        if name.isEmpty || !idEdited {
            name = url.lastPathComponent
        }
    }

    private func add() async {
        guard let folder else { return }
        adding = true
        defer { adding = false }
        var server = Server(id: id, name: name, folder: folder.path)
        server.spawn = spawn
        server.autoStart = autoStart
        if await store.add(server, start: startNow) != nil {
            dismiss()
        }
    }
}
