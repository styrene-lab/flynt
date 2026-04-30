import Foundation

/// Writes shared items as Codyx-compatible .md files into the App Group inbox.
/// The main Codyx app drains this inbox on launch and periodically.
struct InboxWriter {

    private static let appGroupID = "group.io.styrene.codex"
    private static let inboxDir = "codex-inbox"
    private static let assetsDir = "codex-inbox/assets"

    /// Write one or more shared items as a single .md document in the inbox.
    func write(items: [ShareItem], title: String?) throws {
        guard let container = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: Self.appGroupID
        ) else {
            throw InboxError.noAppGroupContainer
        }

        let inboxURL = container.appendingPathComponent(Self.inboxDir)
        try FileManager.default.createDirectory(at: inboxURL, withIntermediateDirectories: true)

        let docID = UUID().uuidString.lowercased()
        var body = ""
        var docTitle = title ?? "Shared item"
        var tags = ["inbox"]

        for item in items {
            switch item {
            case .url(let url, let pageTitle):
                if title == nil, let pageTitle {
                    docTitle = pageTitle
                }
                body += "[\(pageTitle ?? url.absoluteString)](\(url.absoluteString))\n\n"
                tags.append("link")

            case .text(let text):
                if title == nil {
                    // Use first line as title, up to 80 chars
                    let firstLine = text.prefix(while: { $0 != "\n" })
                    docTitle = String(firstLine.prefix(80))
                }
                body += text + "\n\n"

            case .image(let data, let ext):
                let assetsURL = container.appendingPathComponent(Self.assetsDir)
                try FileManager.default.createDirectory(at: assetsURL, withIntermediateDirectories: true)

                let assetName = "\(docID).\(ext)"
                let assetPath = assetsURL.appendingPathComponent(assetName)
                try data.write(to: assetPath, options: .atomic)

                body += "![image](assets/\(assetName))\n\n"
                tags.append("image")
            }
        }

        // Build TOML frontmatter
        let tagsToml = tags.map { "\"\($0)\"" }.joined(separator: ", ")
        let iso8601 = ISO8601DateFormatter().string(from: Date())
        let frontmatter = """
        +++
        title = "\(escapeToml(docTitle))"
        tags = [\(tagsToml)]
        imported_at = "\(iso8601)"
        +++
        """

        let document = """
        \(frontmatter)

        # \(docTitle)

        \(body.trimmingCharacters(in: .whitespacesAndNewlines))
        """

        // Atomic write: write to temp, then rename into inbox
        let tempURL = inboxURL.appendingPathComponent(".\(docID).tmp")
        let finalURL = inboxURL.appendingPathComponent("\(docID).md")

        try document.write(to: tempURL, atomically: true, encoding: .utf8)
        try FileManager.default.moveItem(at: tempURL, to: finalURL)
    }

    private func escapeToml(_ s: String) -> String {
        s.replacingOccurrences(of: "\\", with: "\\\\")
         .replacingOccurrences(of: "\"", with: "\\\"")
    }
}

enum InboxError: Error, LocalizedError {
    case noAppGroupContainer

    var errorDescription: String? {
        switch self {
        case .noAppGroupContainer:
            return "Could not access App Group container. Ensure 'group.io.styrene.codex' is configured."
        }
    }
}
