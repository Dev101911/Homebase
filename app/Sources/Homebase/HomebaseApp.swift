import AppKit
import SwiftUI

@main
struct HomebaseApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @State private var store = Store()

    var body: some Scene {
        MenuBarExtra {
            MenuView()
                .environment(store)
        } label: {
            MenuBarIcon(overall: store.overall)
        }
        .menuBarExtraStyle(.window)

        Settings {
            SettingsView()
                .environment(store)
        }

        Window("Server Log", id: "log") {
            LogWindow()
                .environment(store)
        }
        .defaultSize(width: 760, height: 480)
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        // Menu bar only; a Dock icon appears while a window is open.
        NSApp.setActivationPolicy(.accessory)
    }
}

/// A house: filled when servers run, with a warning or error symbol when one needs you.
struct MenuBarIcon: View {
    var overall: Store.Overall

    var body: some View {
        Image(systemName: symbol)
            .accessibilityLabel(accessibility)
    }

    private var symbol: String {
        switch overall {
        case .agentDown: "house.slash"
        case .idle: "house"
        case .allRunning, .someStopped: "house.fill"
        case .attention: "exclamationmark.triangle.fill"
        case .failed: "xmark.octagon.fill"
        }
    }

    private var accessibility: String {
        switch overall {
        case .agentDown: "Homebase: agent not running"
        case .idle: "Homebase: no servers running"
        case .allRunning: "Homebase: all servers running"
        case .someStopped: "Homebase: some servers stopped"
        case .attention: "Homebase: a server needs attention"
        case .failed: "Homebase: a server failed"
        }
    }
}

/// The coloured dot for a server's state.
struct StateDot: View {
    var state: ServerState?

    var body: some View {
        Circle()
            .fill(color)
            .frame(width: 8, height: 8)
            .accessibilityHidden(true)
    }

    private var color: Color {
        switch state {
        case .running: .green
        case .starting, .backingOff: .yellow
        case .needsAttention: .orange
        case .failed: .red
        case .stopped, nil: .secondary.opacity(0.5)
        }
    }
}
