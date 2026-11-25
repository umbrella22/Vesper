import { ExampleModel, ExampleDownloadItem } from '../model/ExampleModel';
import { examplePalette, ExamplePalette } from '../model/ExampleSupport';
import { sheetAfterAction } from '../model/ExampleShell';
import { ExampleAction, ExampleHeading } from './ExampleControls';
import { DownloadTaskCard } from './DownloadTaskCard';

/** Task list on the page; the new-download form lives in a half-modal sheet. */
@Component
export struct DownloadsPanel {
  @ObjectLink model: ExampleModel;
  @Prop palette: ExamplePalette = examplePalette(false);
  @State sheet: boolean = false;
  @State formError: string = '';
  private available(): boolean { return this.model.ready && !this.model.busy; }
  private text(key: string): string { return this.model.text('example_' + key); }
  private submit(): void {
    this.formError = '';
    this.model.createDownload((succeeded: boolean): void => {
      if (!this.sheet) { return; }
      this.sheet = sheetAfterAction('download', succeeded).length > 0;
      this.formError = succeeded || !this.model.messageFailed ? '' : this.model.message;
    });
  }
  @Builder form() {
    Column({ space: 16 }) {
      TextInput({ text: this.model.downloadAddress, placeholder: 'https://example.com/video.mp4' }).constraintSize({ minHeight: 48 }).fontSize(14)
        .backgroundColor(this.palette.field).fontColor(this.palette.title).placeholderColor(this.palette.secondary).borderRadius(12).enterKeyType(EnterKeyType.Go)
        .onChange((value: string): void => { this.model.downloadAddress = value; this.formError = ''; }).accessibilityText(this.text('download_url'))
        .onSubmit((): void => { if (this.available()) { this.submit(); } })
      Select([{ value: this.text('auto') }, { value: 'MP4' }, { value: 'HLS' }, { value: 'DASH' }])
        .selected(['auto', 'progressive', 'hls', 'dash'].indexOf(this.model.downloadProtocol))
        .value(this.model.downloadProtocol === 'auto' ? this.text('auto') : this.model.downloadProtocol === 'progressive' ? 'MP4' : this.model.downloadProtocol.toUpperCase()).constraintSize({ minHeight: 44 })
        .fontColor(this.palette.title).backgroundColor(this.palette.field)
        .onSelect((index: number): void => { this.model.downloadProtocol = ['auto', 'progressive', 'hls', 'dash'][index]; }).accessibilityText(this.text('download_type'))
      Text(this.text('download_note')).fontSize(12).lineHeight(18).fontColor(this.palette.secondary).width('100%')
      if (this.formError.length > 0) {
        Text(this.formError).fontSize(13).lineHeight(19).fontColor(this.palette.danger).width('100%')
      }
      ExampleAction({ title: this.text('download_video'), icon: $r('sys.symbol.download'), palette: this.palette, primary: true, isEnabled: this.available(),
        onAction: (): void => { this.submit(); } }).width('100%')
    }.padding({ left: 20, right: 20, bottom: 32 }).width('100%')
  }
  build() {
    Column({ space: 24 }) {
      ExampleHeading({ title: this.text('offline_title'), detail: this.text('offline_detail'), palette: this.palette })
      ExampleAction({ title: this.text('new_download'), icon: $r('sys.symbol.plus'), palette: this.palette, primary: true, isEnabled: this.available(),
        onAction: (): void => { this.formError = ''; this.sheet = true; } }).width('100%')
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
      .bindSheet($$this.sheet, this.form(), { height: SheetSize.MEDIUM, dragBar: true, showClose: true,
        keyboardAvoidMode: SheetKeyboardAvoidMode.TRANSLATE_AND_SCROLL, title: { title: this.text('new_download') },
        backgroundColor: this.palette.top, onDisappear: (): void => { this.formError = ''; } })
  }
}
