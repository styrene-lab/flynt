import UIKit
import SwiftUI
import UniformTypeIdentifiers

@objc(ShareViewController)
class ShareViewController: UIViewController {

    private var items: [ShareItem] = []

    override func viewDidLoad() {
        super.viewDidLoad()
        extractSharedItems { [weak self] extracted in
            guard let self else { return }
            self.items = extracted
            let hostingController = UIHostingController(
                rootView: ShareSheetView(
                    items: extracted,
                    onSave: { title in self.save(title: title) },
                    onCancel: { self.cancel() }
                )
            )
            self.addChild(hostingController)
            self.view.addSubview(hostingController.view)
            hostingController.view.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                hostingController.view.topAnchor.constraint(equalTo: self.view.topAnchor),
                hostingController.view.bottomAnchor.constraint(equalTo: self.view.bottomAnchor),
                hostingController.view.leadingAnchor.constraint(equalTo: self.view.leadingAnchor),
                hostingController.view.trailingAnchor.constraint(equalTo: self.view.trailingAnchor),
            ])
            hostingController.didMove(toParent: self)
        }
    }

    // MARK: - Extract shared content from NSExtensionItem providers

    private func extractSharedItems(completion: @escaping ([ShareItem]) -> Void) {
        guard let extensionItems = extensionContext?.inputItems as? [NSExtensionItem] else {
            completion([])
            return
        }

        var results: [ShareItem] = []
        let group = DispatchGroup()

        for extensionItem in extensionItems {
            guard let providers = extensionItem.attachments else { continue }
            for provider in providers {
                if provider.hasItemConformingToTypeIdentifier(UTType.url.identifier) {
                    group.enter()
                    provider.loadItem(forTypeIdentifier: UTType.url.identifier) { item, _ in
                        if let url = item as? URL {
                            results.append(.url(url, title: extensionItem.attributedContentText?.string))
                        }
                        group.leave()
                    }
                } else if provider.hasItemConformingToTypeIdentifier(UTType.plainText.identifier) {
                    group.enter()
                    provider.loadItem(forTypeIdentifier: UTType.plainText.identifier) { item, _ in
                        if let text = item as? String {
                            results.append(.text(text))
                        }
                        group.leave()
                    }
                } else if provider.hasItemConformingToTypeIdentifier(UTType.image.identifier) {
                    group.enter()
                    provider.loadItem(forTypeIdentifier: UTType.image.identifier) { item, _ in
                        if let url = item as? URL, let data = try? Data(contentsOf: url) {
                            let ext = url.pathExtension.isEmpty ? "png" : url.pathExtension
                            results.append(.image(data, fileExtension: ext))
                        } else if let image = item as? UIImage, let data = image.pngData() {
                            results.append(.image(data, fileExtension: "png"))
                        }
                        group.leave()
                    }
                }
            }
        }

        group.notify(queue: .main) {
            completion(results)
        }
    }

    // MARK: - Save and dismiss

    private func save(title: String) {
        let writer = InboxWriter()
        do {
            try writer.write(items: items, title: title.isEmpty ? nil : title)
        } catch {
            NSLog("[CodexShare] Failed to write to inbox: \(error)")
        }
        extensionContext?.completeRequest(returningItems: nil)
    }

    private func cancel() {
        extensionContext?.cancelRequest(
            withError: NSError(domain: "io.styrene.codex.share-extension", code: 0)
        )
    }
}

// MARK: - SwiftUI Share Sheet

struct ShareSheetView: View {
    let items: [ShareItem]
    let onSave: (String) -> Void
    let onCancel: () -> Void
    @State private var title: String = ""

    var body: some View {
        NavigationView {
            Form {
                Section {
                    TextField("Title (optional)", text: $title)
                }
                Section(header: Text("Sharing")) {
                    ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                        HStack {
                            Image(systemName: item.iconName)
                                .foregroundColor(.accentColor)
                            Text(item.preview)
                                .lineLimit(2)
                                .font(.subheadline)
                        }
                    }
                }
            }
            .navigationTitle("Save to Codex")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", action: onCancel)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { onSave(title) }
                        .bold()
                }
            }
        }
    }
}
