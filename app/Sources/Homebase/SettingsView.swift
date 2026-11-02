import SwiftUI

/// A sidebar of servers and a form for the selected one, plus General.
struct SettingsView: View {
    @Environment(Store.self) private var store

    var body: some View {
        @Bindable var store = store
        NavigationSplitView {
            List(selection: $store.selection) {
                Section("Servers") {
                    ForEach(store.servers) { server in
                        HStack(spacing: 8) {
                            StateDot(state: store.status(for: server.id)?.state)
                            Text(server.name)
                        }
                        .tag(server.id)
                    }
                }
                Section {
                    Label("General", systemImage: "gearshape").tag(Store.generalTab)
                }
            }
            .navigationSplitViewColumnWidth(min: 180, ideal: 200)
            .safeAreaInset(edge: .bottom) {
                HStack {
                    Button {
                        store.showingAddSheet = true
                    } label: {
                        Label("Add Server", systemImage: "plus")
                    }
                    .buttonStyle(.borderless)
                    .disabled(!store.agentRunning)
                    Spacer()
                }
                .padding(10)
            }
        } detail: {
            detail
        }
        .frame(minWidth: 760, minHeight: 560)
        .sheet(isPresented: $store.showingAddSheet) {
            AddServerSheet().environment(store)
        }
        .onAppear {
            DockIcon.windowOpened()
            if store.selection == nil {
                store.selection = store.servers.first?.id ?? Store.generalTab
            }
        }
        .onDisappear { DockIcon.windowClosed() }
    }

    @ViewBuilder
    private var detail: some View {
        if let error = store.configError {
            ErrorBanner(message: error) {}
                .padding()
        }
        if store.selection == Store.generalTab {
            GeneralView()
        } else if let id = store.selection, let server = store.server(for: id) {
            ServerDetail(saved: server)
                .id(server.id)
        } else if !store.agentRunning {
            ContentUnavailableView(
                "Agent not running", systemImage: "house.slash",
                description: Text("Homebase starts it automatically. If it doesn't, run hb doctor in Terminal."))
        } else {
            ContentUnavailableView(
                "No server selected", systemImage: "server.rack",
                description: Text("Choose a server, or add one for a project folder."))
        }
    }
}
