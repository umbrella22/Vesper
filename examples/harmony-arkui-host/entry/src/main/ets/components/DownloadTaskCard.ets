import { ExampleModel, ExampleDownloadItem } from '../model/ExampleModel';
import { downloadCanPause, downloadPercent, examplePalette, ExamplePalette, formatBytes } from '../model/ExampleSupport';
import { ExampleAction, ExampleIconAction } from './ExampleControls';

@Component
export struct DownloadTaskCard {
  @ObjectLink model: ExampleModel;
  @ObjectLink entry: ExampleDownloadItem;
  @Prop palette: ExamplePalette = examplePalette(false);
  private available(): boolean { return this.model.ready && !this.model.busy; }
  private text(key: string): string { return this.model.text('example_' + key); }
  private exportable(): boolean { return this.available() && this.entry.snapshot.exportEligibility?.available === true; }
  build() {
    Column({ space: 12 }) {
      Text(this.entry.snapshot.title || this.text('downloaded_video')).fontSize(17).fontWeight(FontWeight.Medium)
        .fontColor(this.palette.title).width('100%').maxLines(3).textOverflow({ overflow: TextOverflow.Ellipsis })
      Text(this.text('download_' + this.entry.snapshot.status)).fontSize(13)
        .fontColor(this.entry.snapshot.status === 'Failed' ? this.palette.danger : this.palette.accent).width('100%')
      if (downloadPercent(this.entry.snapshot) !== undefined) {
        Progress({ value: downloadPercent(this.entry.snapshot) ?? 0, total: 100, type: ProgressType.Linear })
          .color(this.palette.accent).backgroundColor(this.palette.field).width('100%')
      }
      Text(formatBytes(this.entry.snapshot.receivedBytes) + ' / ' + formatBytes(this.entry.snapshot.totalBytes))
        .fontFeature('tnum').fontSize(13).fontColor(this.palette.secondary).width('100%')
      if (this.entry.snapshot.status !== 'Completed') {
        Text(this.text(this.entry.snapshot.resumeState === 'source-refresh-required' ? 'resume_refresh' :
          Number(this.entry.snapshot.reusableBytes ?? 0) > 0 ? 'resume_verify' : 'resume_restart')).fontSize(13).fontColor(this.palette.secondary).width('100%')
        if (Number(this.entry.snapshot.reusableBytes ?? 0) > 0) {
          Text(this.text('resume_reusable') + ': ' + formatBytes(this.entry.snapshot.reusableBytes)).fontSize(12).fontColor(this.palette.secondary).width('100%')
        }
      }
      if (this.entry.snapshot.status === 'Completed') {
        Row({ space: 8 }) {
          ExampleAction({ title: this.text('play_offline'), icon: $r('sys.symbol.play_fill'), palette: this.palette, primary: true,
            isEnabled: this.available(), onAction: (): void => { this.model.playDownload(this.entry.snapshot); } }).layoutWeight(1)
          ExampleIconAction({ title: this.text('save'), icon: $r('sys.symbol.square_and_arrow_down'), palette: this.palette,
            isEnabled: this.exportable(), onAction: (): void => { this.model.exportDownload(this.entry.snapshot, false); } })
          ExampleIconAction({ title: this.text('share'), icon: $r('sys.symbol.share'), palette: this.palette,
            isEnabled: this.exportable(), onAction: (): void => { this.model.exportDownload(this.entry.snapshot, true); } })
        }.width('100%')
        if (this.entry.snapshot.exportEligibility?.reason === 'size-limit') {
          Text(this.text('export_too_large')).fontSize(13).fontColor(this.palette.secondary).width('100%')
        }
      }
      Row({ space: 8 }) {
        if (downloadCanPause(this.entry.snapshot.status)) {
          ExampleIconAction({ title: this.text('pause_download'), icon: $r('sys.symbol.pause'), palette: this.palette,
            isEnabled: this.available(), onAction: (): void => { this.model.pauseDownload(this.entry.snapshot); } })
        }
        if (this.entry.snapshot.status === 'Paused' || this.entry.snapshot.status === 'Failed') {
          ExampleIconAction({ title: this.text('resume_download'), icon: $r('sys.symbol.play_fill'), palette: this.palette,
            isEnabled: this.available(), onAction: (): void => { this.model.resumeDownload(this.entry.snapshot); } })
        }
        Blank()
        ExampleIconAction({ title: this.text('delete'), icon: $r('sys.symbol.trash'), palette: this.palette, danger: true,
          isEnabled: this.available() && this.model.canRemoveDownload(this.entry.snapshot.taskId), onAction: (): void => { this.model.removeDownload(this.entry.snapshot); } })
      }.width('100%')
      if (!this.model.canRemoveDownload(this.entry.snapshot.taskId)) {
        Text(this.text('download_in_use')).fontSize(13).fontColor(this.palette.secondary).width('100%')
      }
    }.padding(16).width('100%').backgroundColor(this.palette.surface).borderRadius(18)
  }
}
