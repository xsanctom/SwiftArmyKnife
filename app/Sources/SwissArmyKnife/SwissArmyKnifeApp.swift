import SwiftUI

@main
struct SwissArmyKnifeApp: App {
    @StateObject private var model = AppModel()

    init() {
        // Resolve ffmpeg/ffprobe. A bundled copy under Resources/bin wins if
        // present; otherwise use Homebrew's absolute path (a GUI app launched
        // via Finder/`open` doesn't inherit the shell PATH, so we can't rely
        // on a bare "ffmpeg").
        let ffmpeg = Self.resolve("ffmpeg")
        let ffprobe = Self.resolve("ffprobe")
        Engine.initialize(ffmpeg: ffmpeg, ffprobe: ffprobe)
    }

    /// Bundled copy if present, else the first existing known install path.
    private static func resolve(_ name: String) -> String {
        if let bundled = Bundle.main.url(forResource: name, withExtension: nil, subdirectory: "bin")?.path {
            return bundled
        }
        let candidates = ["/opt/homebrew/bin/\(name)", "/usr/local/bin/\(name)"]
        return candidates.first { FileManager.default.isExecutableFile(atPath: $0) }
            ?? "/opt/homebrew/bin/\(name)"
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(model)
        }
        .windowResizability(.contentSize)
        .defaultSize(width: 400, height: 400)
    }
}
