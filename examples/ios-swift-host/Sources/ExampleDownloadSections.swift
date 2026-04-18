import SwiftUI
import VesperPlayerKit

struct ExampleDownloadHeader: View {
    let palette: ExampleHostPalette
    let isDownloadExportPluginInstalled: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Vesper Download")
                .font(.system(size: 38, weight: .black, design: .rounded))
                .foregroundStyle(palette.title)

            Text(ExampleI18n.downloadHeaderSubtitle)
                .font(.body)
                .foregroundStyle(palette.body)
                .lineSpacing(5)

            Text(
                isDownloadExportPluginInstalled
                    ? ExampleI18n.downloadExportPluginReady
                    : ExampleI18n.downloadExportPluginMissing
            )
                .font(.caption)
                .foregroundStyle(palette.body)
                .lineSpacing(4)
        }
    }
}

struct ExamplePendingDownloadTask: Identifiable, Equatable {
    let id: String
    let assetId: String
    let label: String
    let sourceUri: String
}

struct ExampleDownloadCreateSection: View {
    let palette: ExampleHostPalette
    @Binding var remoteUrl: String
    let message: String?
    let onUseHlsDemo: () -> Void
    let onUseDashDemo: () -> Void
    let onCreateRemote: () -> Void

    var body: some View {
        ExampleSectionShell(
            palette: palette,
            title: ExampleI18n.downloadCreateTitle,
            subtitle: ExampleI18n.downloadCreateSubtitle
        ) {
            VStack(alignment: .leading, spacing: 14) {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 10) {
                        actionButton(ExampleI18n.downloadHlsDemo, action: onUseHlsDemo)
                        actionButton(ExampleI18n.downloadDashDemo, action: onUseDashDemo)
                    }
                }

                if let message {
                    Text(message)
                        .font(.caption)
                        .foregroundStyle(Color.red.opacity(0.92))
                        .lineSpacing(4)
                }

                TextField(ExampleI18n.downloadRemoteUrl, text: $remoteUrl)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .padding(.horizontal, 14)
                    .padding(.vertical, 12)
                    .background(palette.fieldBackground, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                    .foregroundStyle(palette.fieldText)

                Button(action: onCreateRemote) {
                    Text(ExampleI18n.downloadCreateRemoteTask)
                        .font(.headline)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 14)
                }
                .buttonStyle(.plain)
                .background(palette.primaryAction, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                .foregroundStyle(.white)
            }
        }
    }

    @ViewBuilder
    private func actionButton(
        _ title: String,
        action: @escaping () -> Void
    ) -> some View {
        Button(title, action: action)
            .buttonStyle(.plain)
            .font(.subheadline.weight(.semibold))
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(.white.opacity(0.08), in: Capsule())
            .foregroundStyle(palette.title)
    }
}

struct ExampleDownloadTasksSection: View {
    let palette: ExampleHostPalette
    let tasks: [VesperDownloadTaskSnapshot]
    let pendingTasks: [ExamplePendingDownloadTask]
    let isDownloadExportPluginInstalled: Bool
    let savingTaskIds: Set<VesperDownloadTaskId>
    let exportProgressByTaskId: [VesperDownloadTaskId: Float]
    let onPrimaryAction: (VesperDownloadTaskSnapshot) -> Void
    let onSaveToPhotos: (VesperDownloadTaskSnapshot) -> Void
    let onRemoveTask: (VesperDownloadTaskSnapshot) -> Void

    var body: some View {
        let visibleTasks = tasks.filter { task in
            task.state != .removed
        }
        ExampleSectionShell(
            palette: palette,
            title: ExampleI18n.downloadTasksTitle,
            subtitle: ExampleI18n.downloadTasksSubtitle
        ) {
            if visibleTasks.isEmpty && pendingTasks.isEmpty {
                Text(ExampleI18n.downloadEmpty)
                    .font(.footnote)
                    .foregroundStyle(palette.body)
            } else {
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(Array(pendingTasks.reversed())) { task in
                        ExamplePendingDownloadTaskRow(
                            palette: palette,
                            task: task
                        )
                    }
                    ForEach(Array(visibleTasks.reversed()), id: \.taskId) { task in
                        ExampleDownloadTaskRow(
                            palette: palette,
                            task: task,
                            isDownloadExportPluginInstalled: isDownloadExportPluginInstalled,
                            isSaving: savingTaskIds.contains(task.taskId),
                            exportProgress: exportProgressByTaskId[task.taskId],
                            onPrimaryAction: { onPrimaryAction(task) },
                            onSaveToPhotos: { onSaveToPhotos(task) },
                            onRemoveTask: { onRemoveTask(task) }
                        )
                    }
                }
            }
        }
    }
}

private struct ExamplePendingDownloadTaskRow: View {
    let palette: ExampleHostPalette
    let task: ExamplePendingDownloadTask

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(task.label)
                .font(.body.weight(.semibold))
                .foregroundStyle(palette.title)
                .lineLimit(1)

            Text(ExampleI18n.downloadTaskMeta(task.assetId, ExampleI18n.downloadStatePreparing))
                .font(.caption)
                .foregroundStyle(palette.body)
                .lineSpacing(4)

            ProgressView()
                .tint(palette.primaryAction)

            Text(ExampleI18n.downloadPendingTaskDetails)
                .font(.caption)
                .foregroundStyle(palette.body)
                .lineSpacing(4)

            Text(ExampleI18n.downloadPendingSourceUri(task.sourceUri))
                .font(.caption)
                .foregroundStyle(palette.body)
                .lineSpacing(4)
                .lineLimit(2)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .background(palette.fieldBackground, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .stroke(palette.sectionStroke, lineWidth: 1)
        )
    }
}

private struct ExampleDownloadTaskRow: View {
    let palette: ExampleHostPalette
    let task: VesperDownloadTaskSnapshot
    let isDownloadExportPluginInstalled: Bool
    let isSaving: Bool
    let exportProgress: Float?
    let onPrimaryAction: () -> Void
    let onSaveToPhotos: () -> Void
    let onRemoveTask: () -> Void

    var body: some View {
        let requiresExport =
            task.source.contentFormat == .hlsSegments ||
            task.source.contentFormat == .dashSegments
        let saveButtonVisuallyUnavailable =
            requiresExport && !isDownloadExportPluginInstalled && !isSaving
        VStack(alignment: .leading, spacing: 10) {
            Text(task.source.source.label)
                .font(.body.weight(.semibold))
                .foregroundStyle(palette.title)
                .lineLimit(1)

            Text(ExampleI18n.downloadTaskMeta(task.assetId, downloadStateLabel(task.state)))
                .font(.caption)
                .foregroundStyle(palette.body)
                .lineSpacing(4)

            Text(downloadProgressSummary(task))
                .font(.caption)
                .foregroundStyle(palette.body)
                .lineSpacing(4)

            if isSaving, let exportProgress {
                ProgressView(value: Double(max(Float(0), min(Float(1), exportProgress))))
                    .tint(palette.primaryAction)
                Text(
                    ExampleI18n.downloadExportProgress(
                        Int(max(Float(0), min(Float(1), exportProgress)) * 100.0)
                    )
                )
                    .font(.caption)
                    .foregroundStyle(palette.body)
                    .lineSpacing(4)
            }

            if let completedPath = task.assetIndex.completedPath, !completedPath.isEmpty {
                Text(ExampleI18n.downloadCompletedPath(completedPath))
                    .font(.caption)
                    .foregroundStyle(palette.body)
                    .lineSpacing(4)
                    .lineLimit(2)
            }

            if let message = task.error?.message, !message.isEmpty {
                Text(ExampleI18n.downloadErrorMessage(message))
                    .font(.caption)
                    .foregroundStyle(Color.red.opacity(0.92))
                    .lineSpacing(4)
                    .lineLimit(2)
            }

            HStack(spacing: 10) {
                if let primaryActionLabel = downloadPrimaryActionLabel(task.state) {
                    Button(action: onPrimaryAction) {
                        Text(primaryActionLabel)
                            .font(.subheadline.weight(.semibold))
                            .padding(.horizontal, 14)
                            .padding(.vertical, 10)
                    }
                    .buttonStyle(.plain)
                    .background(palette.primaryAction, in: Capsule())
                    .foregroundStyle(.white)
                    .disabled(isSaving)
                }

                if let completedPath = task.assetIndex.completedPath, !completedPath.isEmpty {
                    Button(action: onSaveToPhotos) {
                        Text(
                            isSaving && exportProgress != nil
                                ? ExampleI18n.downloadExporting
                                : ExampleI18n.downloadSaveToPhotos
                        )
                            .font(.subheadline.weight(.semibold))
                            .padding(.horizontal, 14)
                            .padding(.vertical, 10)
                    }
                    .buttonStyle(.plain)
                    .background(.white.opacity(0.08), in: Capsule())
                    .foregroundStyle(
                        saveButtonVisuallyUnavailable
                            ? palette.body.opacity(0.55)
                            : palette.title
                    )
                    .disabled(isSaving)
                }

                Button(action: onRemoveTask) {
                    Text(ExampleI18n.downloadRemoveTask)
                        .font(.subheadline.weight(.semibold))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 10)
                }
                .buttonStyle(.plain)
                .foregroundStyle(palette.title)
                .disabled(isSaving)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .background(palette.fieldBackground, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .stroke(palette.sectionStroke, lineWidth: 1)
        )
    }
}
