import SwiftUI

/// Library tab: scenario collections, each opening a second-level page.
struct ExampleLibraryPage<DolbyCatalog: View>: View {
    let dolbyPresets: [ExampleDolbyAcceptancePreset]
    let onPlayItem: (String) -> Void
    let onAddItemToQueue: (String) -> Void
    @ViewBuilder let dolbyCatalog: () -> DolbyCatalog

    var body: some View {
        List {
            Section {
                Text(ExampleI18n.librarySubtitle)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets(top: 0, leading: 4, bottom: 4, trailing: 4))
            }
            Section(ExampleI18n.libraryCollections) {
                ForEach(ExampleLibraryCollection.allCases) { collection in
                    NavigationLink(value: collection) {
                        ExampleShellRowLabel(
                            title: collection.title,
                            subtitle: collection.summary,
                            systemImage: collection.systemImage,
                            trailingText: ExampleI18n.libraryItemCount(
                                exampleLibraryItemCount(collection, dolbyPresets: dolbyPresets)
                            )
                        )
                    }
                    .accessibilityIdentifier("library.collection.\(collection.rawValue)")
                }
            }
        }
        .navigationDestination(for: ExampleLibraryCollection.self) { collection in
            collectionDestination(collection)
        }
        #if DEBUG
        .navigationDestination(item: $debugLaunchCollection) { collection in
            collectionDestination(collection)
        }
        .onAppear {
            if let collection = ExampleDebugLaunch.consumeCollection() {
                debugLaunchCollection = collection
            }
        }
        #endif
    }

    #if DEBUG
    @State private var debugLaunchCollection: ExampleLibraryCollection?
    #endif

    @ViewBuilder
    private func collectionDestination(_ collection: ExampleLibraryCollection) -> some View {
        if collection.isDolbyAcceptance {
            ScrollView {
                dolbyCatalog()
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
            }
            .navigationTitle(collection.title)
            .navigationBarTitleDisplayMode(.inline)
        } else {
            ExampleLibraryCollectionPage(
                collection: collection,
                onPlayItem: onPlayItem,
                onAddItemToQueue: onAddItemToQueue
            )
        }
    }
}

struct ExampleLibraryCollectionPage: View {
    let collection: ExampleLibraryCollection
    let onPlayItem: (String) -> Void
    let onAddItemToQueue: (String) -> Void

    var body: some View {
        List {
            Section {
                ForEach(exampleLibraryItems(for: collection)) { item in
                    Button {
                        onPlayItem(item.id)
                    } label: {
                        ExampleShellRowLabel(
                            title: item.title,
                            subtitle: item.subtitle,
                            systemImage: "play.circle"
                        )
                    }
                    .swipeActions(edge: .trailing) {
                        Button {
                            onAddItemToQueue(item.id)
                        } label: {
                            Label(ExampleI18n.dolbyAcceptanceAddToQueue, systemImage: "text.badge.plus")
                        }
                    }
                    .contextMenu {
                        Button {
                            onAddItemToQueue(item.id)
                        } label: {
                            Label(ExampleI18n.dolbyAcceptanceAddToQueue, systemImage: "text.badge.plus")
                        }
                    }
                }
            } footer: {
                Text(collection.summary)
            }
        }
        .navigationTitle(collection.title)
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// Settings sheet. Appearance is the only host preference today.
struct ExampleSettingsSheet: View {
    let themeMode: ExampleThemeMode
    let onThemeModeChange: (ExampleThemeMode) -> Void
    let onDismiss: () -> Void

    var body: some View {
        NavigationStack {
            List {
                Section(ExampleI18n.settingsAppearance) {
                    Picker(
                        ExampleI18n.themeTitle,
                        selection: Binding(get: { themeMode }, set: onThemeModeChange)
                    ) {
                        ForEach(ExampleThemeMode.allCases) { mode in
                            Label(mode.title, systemImage: mode.systemImage).tag(mode)
                        }
                    }
                    .pickerStyle(.inline)
                    .labelsHidden()
                }
                Section {
                } footer: {
                    Text(ExampleI18n.settingsAbout)
                        .frame(maxWidth: .infinity)
                        .padding(.bottom, 12)
                }
            }
            .navigationTitle(ExampleI18n.shellSettings)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button(ExampleI18n.shellDone, action: onDismiss)
                }
            }
        }
    }
}
