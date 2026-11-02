import SwiftUI

/// The agent, the hb command, start at login, a doctor run and About.
struct GeneralView: View {
    @Environment(Store.self) private var store
    @State private var hbMessage: String?
    @State private var serviceInstalled: Bool?
    @State private var serviceMessage: String?
    @State private var doctorOutput: String?
    @State private var runningDoctor = false

    var body: some View {
        Form {
            Section("Agent") {
                LabeledContent("Status") {
                    HStack(spacing: 6) {
                        StateDot(state: store.agentRunning ? .running : .stopped)
                        Text(store.agentRunning ? "Running" : "Not running")
                    }
                }
                if let info = store.agentInfo {
                    LabeledContent("Version", value: "hbd \(info.version) · pid \(info.pid)")
                }
                LabeledContent("Socket") {
                    Text(store.client.socketPath.abbreviatingHome)
                        .textSelection(.enabled)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                Text("Quitting Homebase leaves the agent and your servers running.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section {
                Toggle("Start the agent at login", isOn: Binding(
                    get: { serviceInstalled ?? false },
                    set: { on in Task { await setService(on) } }))
                    .disabled(serviceInstalled == nil)
                if let serviceMessage {
                    Text(serviceMessage).font(.caption).foregroundStyle(.secondary)
                }
            } header: {
                Text("Login")
            } footer: {
                Text("Installs a LaunchAgent that starts hbd at login and brings up servers marked to start with Start All.")
                    .foregroundStyle(.secondary)
            }

            Section("Command line") {
                LabeledContent("hb") {
                    Button("Install the hb Command") { installHB() }
                }
                if let hbMessage {
                    Text(hbMessage).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                }
                LabeledContent("For AI agents") {
                    Button("Copy Setup Prompt") {
                        Tools.copy("Run `hb guide` and follow it to check this Mac with `hb doctor` and manage my Remote Control servers with `hb`. Tell me anything I need to do myself.")
                    }
                }
            }

            Section("Diagnostics") {
                HStack {
                    Button("Run Doctor") { Task { await runDoctor() } }
                        .disabled(runningDoctor)
                    if runningDoctor { ProgressView().controlSize(.small) }
                }
                if let doctorOutput {
                    Text(doctorOutput)
                        .font(.system(.caption, design: .monospaced))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }

            Section("About") {
                LabeledContent("Homebase", value: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "development build")
                Text("Runs your own Claude Code Remote Control servers. Homebase isn't affiliated with Anthropic, and never signs in for you or touches your credentials. Apache-2.0, © 2026 TulipByte Studio.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Link("github.com/Dev101911/Homebase", destination: URL(string: "https://github.com/Dev101911/Homebase")!)
            }
        }
        .formStyle(.grouped)
        .navigationTitle("General")
        .task { await loadService() }
    }

    private func installHB() {
        do {
            let link = try Tools.installCommandLineTool()
            hbMessage = "Linked \(link.path.abbreviatingHome). Make sure ~/.local/bin is on your PATH."
        } catch {
            hbMessage = error.localizedDescription
        }
    }

    private func loadService() async {
        guard let out = await Tools.runHB(["service", "status", "--json"]),
              let json = try? JSONSerialization.jsonObject(with: Data(out.stdout.utf8)) as? [String: Any]
        else {
            serviceMessage = "hb isn't available in this build."
            return
        }
        serviceInstalled = (json["installed"] as? Bool ?? false) && (json["loaded"] as? Bool ?? false)
    }

    private func setService(_ on: Bool) async {
        let out = await Tools.runHB(["service", on ? "install" : "uninstall"])
        serviceMessage = out.map { ($0.stdout + $0.stderr).trimmingCharacters(in: .whitespacesAndNewlines) }
        await loadService()
        await store.retryAgent()
    }

    private func runDoctor() async {
        runningDoctor = true
        let out = await Tools.runHB(["doctor"])
        doctorOutput = out.map { ($0.stdout + $0.stderr).trimmingCharacters(in: .whitespacesAndNewlines) }
            ?? "hb isn't available in this build."
        runningDoctor = false
    }
}
