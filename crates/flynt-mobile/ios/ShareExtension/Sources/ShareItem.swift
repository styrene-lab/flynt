import Foundation

/// Represents a single piece of content shared via the iOS Share Sheet.
enum ShareItem {
    case url(URL, title: String?)
    case text(String)
    case image(Data, fileExtension: String)

    var iconName: String {
        switch self {
        case .url: return "link"
        case .text: return "doc.text"
        case .image: return "photo"
        }
    }

    var preview: String {
        switch self {
        case .url(let url, let title):
            return title ?? url.absoluteString
        case .text(let text):
            return String(text.prefix(120))
        case .image(_, let ext):
            return "Image (.\(ext))"
        }
    }
}
