import AppKit
import Foundation

/// Helpers that leave the app: the bundled hb, Terminal and Finder.
enum Tools {
    /// hb inside the app bundle, else next to the running binary (swift run), else on PATH.
    static var hbURL: URL? {
        let fm = FileManager.default
        let candidates = [
            Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/hb"),
            Bundle.main.executableURL?.deletingLastPathComponent().appendingPathComponent("hb"),
            fm.homeDirectoryForCurrentUser.appendingPathComponent(".local/bin/hb"),
            URL(fileURLWithPath: "/opt/homebrew/bin/hb"),
            URL(fileURLWithPath: "/usr/local/bin/hb"),
        ].compactMap { $0 }
        return candidates.first { fm.isExecutableFile(atPath: $0.path) }
    }

    struct Output: Sendable {
        var status: Int32
        var stdout: String
        var stderr: String
    }

    /// Runs the bundled hb off the main thread.
    static func runHB(_ arguments: [String]) async -> Output? {
        guard let hb = hbURL else { return nil }
        return await Task.detached {
            let process = Process()
            process.executableURL = hb
            process.arguments = arguments
            let out = Pipe()
            let err = Pipe()
            process.standardOutput = out
            process.standardError = err
            process.standardInput = FileHandle.nullDevice
            do {
                try process.run()
            } catch {
                return Output(status: -1, stdout: "", stderr: error.localizedDescription)
            }
            let stdout = out.fileHandleForReading.readDataToEndOfFile()
            let stderr = err.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            return Output(
                status: process.terminationStatus,
                stdout: String(decoding: stdout, as: UTF8.self),
                stderr: String(decoding: stderr, as: UTF8.self))
        }.value
    }

    /// Opens Terminal in the folder, where the person runs `claude` to answer
    /// the trust question. Homebase never answers it for them.
    static func openTerminal(at folder: URL) {
        guard let terminal = NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.apple.Terminal")
        else { return }
        let config = NSWorkspace.OpenConfiguration()
        config.activates = true
        NSWorkspace.shared.open([folder], withApplicationAt: terminal, configuration: config)
    }

    static func revealInFinder(_ path: String) {
        let url = URL(fileURLWithPath: path)
        if FileManager.default.fileExists(atPath: path) {
            NSWorkspace.shared.activateFileViewerSelecting([url])
        } else {
            NSWorkspace.shared.open(url.deletingLastPathComponent())
        }
    }

    static func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    /// Asks for a folder. Returns nil when cancelled.
    @MainActor
    static func chooseFolder(startingAt: URL? = nil) -> URL? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.prompt = "Choose"
        panel.message = "Choose the project folder this server runs in."
        panel.directoryURL = startingAt
        NSApp.activate()
        return panel.runModal() == .OK ? panel.url : nil
    }

    /// Links the bundled hb into ~/.local/bin so it's on the PATH.
    static func installCommandLineTool() throws -> URL {
        guard let hb = hbURL else {
            throw AgentError(code: "missing", message: "This copy of Homebase doesn't include hb.")
        }
        let fm = FileManager.default
        let bin = fm.homeDirectoryForCurrentUser.appendingPathComponent(".local/bin", isDirectory: true)
        try fm.createDirectory(at: bin, withIntermediateDirectories: true)
        let link = bin.appendingPathComponent("hb")
        if (try? fm.destinationOfSymbolicLink(atPath: link.path)) != nil || fm.fileExists(atPath: link.path) {
            try fm.removeItem(at: link)
        }
        try fm.createSymbolicLink(at: link, withDestinationURL: hb.resolvingSymlinksInPath())
        return link
    }
}

/// Shows a Dock icon only while a window is open, as the design asks.
@MainActor
enum DockIcon {
    private static var openWindows = 0

    static func windowOpened() {
        openWindows += 1
        NSApp.setActivationPolicy(.regular)
        NSApp.activate()
    }

    static func windowClosed() {
        openWindows = max(0, openWindows - 1)
        if openWindows == 0 {
            NSApp.setActivationPolicy(.accessory)
        }
    }
}
