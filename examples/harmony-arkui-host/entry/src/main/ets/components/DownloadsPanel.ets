import { ExampleModel, ExampleDownloadItem } from '../model/ExampleModel';
import { examplePalette, ExamplePalette } from '../model/ExampleSupport';
import { ExampleAction, ExampleHeading } from './ExampleControls';
import { DownloadTaskCard } from './DownloadTaskCard';

@Component
export struct DownloadsPanel {
  @ObjectLink model: ExampleModel;
  @Prop palette: ExamplePalette = examplePalette(false);
  private available(): boolean { return this.model.ready && !this.model.busy; }
  private text(key: string): string { return this.model.text('example_' + key); }
  build() {
    Column({ space: 24 }) {
      ExampleHeading({ title: this.text('offline_title'), detail: this.text('offline_detail'), palette: this.palette })
      Column({ space: 12 }) {
        Text(this.text('new_download')).fontSize(17).fontWeight(FontWeight.Medium).fontColor(this.palette.title).width('100%')
        TextInput({ text: this.model.downloadAddress, placeholder: 'https://example.com/video.mp4' }).constraintSize({ minHeight: 48 }).fontSize(14)
          .backgroundColor(this.palette.field).fontColor(this.palette.title).placeholderColor(this.palette.secondary).borderRadius(12)
          .onChange((value: string): void => { this.model.downloadAddress = value; }).accessibilityText(this.text('download_url'))
        Row({ space: 12 }) {
          Select([{ value: this.text('auto') }, { value: 'MP4' }, { value: 'HLS' }, { value: 'DASH' }])
            .selected(['auto', 'progressive', 'hls', 'dash'].indexOf(this.model.downloadProtocol))
            .value(this.model.downloadProtocol === 'auto' ? this.text('auto') : this.model.downloadProtocol === 'progressive' ? 'MP4' : this.model.downloadProtocol.toUpperCase()).constraintSize({ minHeight: 44 })
            .fontColor(this.palette.title).backgroundColor(this.palette.field)
            .onSelect((index: number): void => { this.model.downloadProtocol = ['auto', 'progressive', 'hls', 'dash'][index]; }).accessibilityText(this.text('download_type'))
          ExampleAction({ title: this.text('download_video'), icon: $r('sys.symbol.download'), palette: this.palette, primary: true, isEnabled: this.available(), onAction: (): void => { this.model.createDownload(); } }).layoutWeight(1)
        }.width('100%')
        Text(this.text('download_note'))
          .fontSize(12).lineHeight(18).fontColor(this.palette.secondary).width('100%')
      }.padding(16).width('100%').backgroundColor(this.palette.surface).borderRadius(24)
      if (this.model.downloads.length === 0) {
        Column({ space: 8 }) {
          SymbolGlyph($r('sys.symbol.download')).fontSize(44).fontColor([this.palette.secondary]).margin({ bottom: 8 })
          Text(this.text('downloads_empty')).fontSize(18).fontWeight(FontWeight.Medium).fontColor(this.palette.title)
          Text(this.text('downloads_empty_detail')).fontSize(13).fontColor(this.palette.secondary)
        }.width('100%').padding({ top: 40, bottom: 40 })
      }
      ForEach(this.model.downloads, (item: ExampleDownloadItem) => {
        DownloadTaskCard({ model: this.model, entry: item, palette: this.palette })
      }, (item: ExampleDownloadItem) => item.taskId)
    }.padding(20).constraintSize({ maxWidth: 860 }).width('100%').alignItems(HorizontalAlign.Start)
  }
}
