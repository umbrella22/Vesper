import { ExampleModel, ExampleQueueItem } from '../model/ExampleModel';
import { examplePalette, ExamplePalette } from '../model/ExampleSupport';
import { presentSheet, sheetAfterAction } from '../model/ExampleShell';
import { ExampleAction, ExampleIconAction } from './ExampleControls';
import { PlaybackToolsPanel } from './PlaybackToolsPanel';

@Component
export struct PlaybackPanel {
  @Prop contentPadding: number = 20;
  @ObjectLink model: ExampleModel;
  @Prop palette: ExamplePalette = examplePalette(false);
  @State panel: boolean = false;
  @State pane: string = 'open';
  @State openError: string = '';
  private available(): boolean { return this.model.ready && !this.model.busy; }
  private index(): number { return this.model.queue.findIndex((item: ExampleQueueItem): boolean => item.id === this.model.selectedId); }
  private text(resource: Resource): string { return this.getUIContext().getHostContext()?.resourceManager.getStringSync(resource.id) ?? ''; }
  private present(name: string): void {
    const sheet = presentSheet(name);
    if (sheet.length === 0) { return; }
    this.pane = sheet; this.openError = ''; this.panel = true;
  }
  /** Open-media actions close the sheet on success and keep it, with the error, on failure. */
  private settle(sheet: string): (succeeded: boolean) => void {
    return (succeeded: boolean): void => {
      if (!this.panel || this.pane !== sheet) { return; }
      this.panel = sheetAfterAction(sheet, succeeded).length > 0;
      this.openError = succeeded || !this.model.messageFailed ? '' : this.model.message;
    };
  }
  @Builder queueItem(item: ExampleQueueItem, index: number) {
    Row({ space: 4 }) {
      Button({ type: ButtonType.Normal }) {
        Row({ space: 12 }) {
          SymbolGlyph(item.id === this.model.selectedId ? $r('sys.symbol.waveform') : $r('sys.symbol.film')).fontSize(22)
            .fontColor([item.id === this.model.selectedId ? this.palette.accent : this.palette.secondary]).width(40).height(40)
            .backgroundColor(item.id === this.model.selectedId ? this.palette.accentSoft : this.palette.field).borderRadius(10).accessibilityLevel('no')
          Column({ space: 5 }) {
            Text(item.title).fontSize(15).fontWeight(FontWeight.Medium).fontColor(this.palette.title)
              .maxLines(2).textOverflow({ overflow: TextOverflow.Ellipsis }).width('100%')
            Text(item.id === this.model.selectedId ? $r('app.string.example_now_playing') : item.detail).fontSize(12).fontColor(this.palette.secondary).maxLines(2).width('100%')
          }.layoutWeight(1).alignItems(HorizontalAlign.Start)
          SymbolGlyph(item.id === this.model.selectedId ? $r('sys.symbol.checkmark') : $r('sys.symbol.play_fill')).fontSize(16).fontColor([this.palette.secondary]).accessibilityLevel('no')
        }.padding({ left: 12, right: 12, top: 14, bottom: 14 }).width('100%')
      }.layoutWeight(1).constraintSize({ minHeight: 76 }).borderRadius(14).backgroundColor(Color.Transparent)
        .enabled(this.available()).onClick((): void => { this.model.play(item); }).accessibilityText(item.title).accessibilitySelected(item.id === this.model.selectedId)
      if (item.id !== this.model.selectedId && !item.builtin) {
        ExampleIconAction({ title: this.model.text('example_remove_video') + ': ' + item.title, icon: $r('sys.symbol.trash'), palette: this.palette, isEnabled: this.available(),
          onAction: (): void => { this.model.removeQueueItem(item); } })
      }
      if (this.panel && this.pane === 'queue' && item.source.kind === 'remote' && ['progressive', 'dash'].includes(item.source.protocol)) {
        ExampleIconAction({ title: this.model.text('example_preload_video') + ': ' + item.title, icon: $r('sys.symbol.download'), palette: this.palette,
          isEnabled: this.available() && !['queued', 'running'].includes(this.model.preloadStatus), onAction: (): void => { this.model.preloadItem(item); } })
      }
    }.width('100%')
  }
  @Builder sourceAction(title: Resource, icon: Resource, action: () => void) {
    Button({ type: ButtonType.Normal }) {
      Row({ space: 12 }) {
        SymbolGlyph(icon).fontSize(21).fontColor([this.palette.title]).accessibilityLevel('no')
        Text(title).fontSize(14).fontWeight(FontWeight.Medium).fontColor(this.palette.title).maxLines(2).layoutWeight(1)
        SymbolGlyph($r('sys.symbol.chevron_right')).fontSize(14).fontColor([this.palette.secondary]).accessibilityLevel('no')
      }.padding({ left: 16, right: 16, top: 12, bottom: 12 }).width('100%')
    }.width('100%').constraintSize({ minHeight: 52 }).borderRadius(12).backgroundColor(Color.Transparent)
      .enabled(this.available()).accessibilityText(this.text(title)).onClick(action)
  }
  @Builder openBody() {
    Column({ space: 16 }) {
      Column() {
        this.sourceAction($r('app.string.example_choose_file'), $r('sys.symbol.folder'), (): void => { this.model.pickVideo(this.settle('open')); })
        this.sourceAction($r('app.string.example_hls_sample'), $r('sys.symbol.play_fill'), (): void => { this.model.playSample('hls', this.settle('open')); })
        this.sourceAction($r('app.string.example_dash_sample'), $r('sys.symbol.film'), (): void => { this.model.playSample('dash', this.settle('open')); })
      }.width('100%').backgroundColor(this.palette.surface).borderRadius(16)
      Text($r('app.string.example_open_stream')).fontSize(15).fontWeight(FontWeight.Medium).fontColor(this.palette.title).width('100%')
      Text($r('app.string.example_media_hint')).fontSize(13).fontColor(this.palette.secondary).width('100%')
      TextInput({ text: this.model.address, placeholder: 'https://example.com/video.mp4' }).constraintSize({ minHeight: 48 }).fontSize(14)
        .backgroundColor(this.palette.field).fontColor(this.palette.title).placeholderColor(this.palette.secondary).borderRadius(12).enterKeyType(EnterKeyType.Go)
        .onChange((value: string): void => { this.model.address = value; this.openError = ''; }).accessibilityText(this.text($r('app.string.example_media_url')))
        .onSubmit((): void => { if (this.available()) { this.model.openRemote(true, this.settle('open')); } })
      Select([{ value: this.text($r('app.string.example_auto')) }, { value: 'MP4' }, { value: 'HLS' }, { value: 'DASH' }])
        .selected(['auto', 'progressive', 'hls', 'dash'].indexOf(this.model.protocol)).value(this.model.protocol === 'auto' ? this.text($r('app.string.example_auto')) : this.model.protocol === 'progressive' ? 'MP4' : this.model.protocol.toUpperCase())
        .onSelect((index: number): void => { this.model.protocol = ['auto', 'progressive', 'hls', 'dash'][index]; }).accessibilityText(this.text($r('app.string.example_media_type')))
      if (this.openError.length > 0) {
        Text(this.openError).fontSize(13).lineHeight(19).fontColor(this.palette.danger).width('100%')
      }
      Row({ space: 8 }) {
        ExampleAction({ title: this.text($r('app.string.example_open_url')), icon: $r('sys.symbol.play_fill'), palette: this.palette, primary: true, isEnabled: this.available(),
          onAction: (): void => { this.model.openRemote(true, this.settle('open')); } }).layoutWeight(1)
        ExampleIconAction({ title: this.text($r('app.string.example_add_queue')), icon: $r('sys.symbol.plus'), palette: this.palette, isEnabled: this.available(),
          onAction: (): void => { this.model.openRemote(false, this.settle('open')); } })
      }.width('100%')
    }.width('100%')
  }
  @Builder panelBody() {
    Scroll() {
      Column({ space: 16 }) {
        if (this.pane === 'queue') {
          ForEach(this.model.queue, (item: ExampleQueueItem, index: number) => { this.queueItem(item, index) }, (item: ExampleQueueItem): string => item.id)
          Text($r('app.string.example_queue_hint')).fontSize(13).fontColor(this.palette.secondary).width('100%')
        } else if (this.pane === 'open') { this.openBody() }
        else { PlaybackToolsPanel({ model: this.model, palette: this.palette }) }
      }.padding({ left: 20, right: 20, bottom: 32 }).width('100%')
    }.height('100%').width('100%').align(Alignment.TopStart)
  }
  private sheetTitle(): Resource {
    return this.pane === 'queue' ? $r('app.string.example_manage_queue') : this.pane === 'open' ? $r('app.string.example_open_media') : $r('app.string.example_options');
  }
  build() {
    Column({ space: 22 }) {
      ExampleAction({ title: this.text($r('app.string.example_open_media')), icon: $r('sys.symbol.plus'), palette: this.palette, primary: true,
        isEnabled: this.available(), onAction: (): void => { this.present('open'); } }).width('100%')
      Column({ space: 8 }) {
        Row() {
          Text($r('app.string.example_up_next')).fontSize(17).fontWeight(FontWeight.Medium).fontColor(this.palette.title).layoutWeight(1)
          Button($r('app.string.example_manage_queue')).constraintSize({ minHeight: 48 }).fontSize(13).backgroundColor(Color.Transparent).fontColor(this.palette.title)
            .onClick((): void => { this.present('queue'); })
        }.width('100%')
        Column() {
          ForEach(this.model.queuePreview(), (item: ExampleQueueItem, index: number) => { this.queueItem(item, index) }, (item: ExampleQueueItem): string => item.id)
        }.width('100%').backgroundColor(this.palette.surface).borderRadius(18)
        Row({ space: 8 }) {
          ExampleIconAction({ title: this.text($r('app.string.example_previous')), icon: $r('sys.symbol.backward_end_fill'), palette: this.palette,
            isEnabled: this.available() && this.index() > 0, onAction: (): void => { this.model.next(-1); } })
          Text(Math.max(1, this.index() + 1) + ' / ' + this.model.queue.length).fontSize(13).fontColor(this.palette.secondary)
          ExampleIconAction({ title: this.text($r('app.string.example_next')), icon: $r('sys.symbol.forward_end_fill'), palette: this.palette,
            isEnabled: this.available() && this.index() < this.model.queue.length - 1, onAction: (): void => { this.model.next(1); } })
          Blank()
        }.width('100%')
      }.width('100%')
      Column({ space: 8 }) {
        Text($r('app.string.example_system_playback')).fontSize(17).fontWeight(FontWeight.Medium).fontColor(this.palette.title).width('100%')
        Column({ space: 4 }) {
          Row({ space: 12 }) {
            Text($r('app.string.example_background_audio')).fontSize(14).fontColor(this.palette.title).layoutWeight(1)
            Toggle({ type: ToggleType.Switch, isOn: this.model.systemEnabled }).enabled(this.available()).accessibilityText(this.text($r('app.string.example_system_playback')))
              .onChange((value: boolean): void => { this.model.setSystemEnabled(value); })
          }.width('100%').constraintSize({ minHeight: 56 })
          Row({ space: 12 }) {
            Text($r('app.string.example_auto_pip')).fontSize(14).fontColor(this.palette.title).layoutWeight(1)
            Toggle({ type: ToggleType.Switch, isOn: this.model.pipAutoStart }).enabled(this.available()).accessibilityText(this.text($r('app.string.example_auto_pip')))
              .onChange((value: boolean): void => { this.model.setPiPAutoStart(value); })
          }.width('100%').constraintSize({ minHeight: 56 })
          this.sourceAction($r('app.string.example_pip'), $r('sys.symbol.smal_window_playback'), (): void => { this.model.pictureInPicture(); })
          this.sourceAction($r('app.string.example_options'), $r('sys.symbol.gearshape'), (): void => { this.present('tools'); })
        }.padding({ left: 16, right: 16, top: 8, bottom: 8 }).width('100%').backgroundColor(this.palette.surface).borderRadius(18)
      }.width('100%')
    }.padding(this.contentPadding).width('100%').alignItems(HorizontalAlign.Start)
      .bindSheet($$this.panel, this.panelBody(), { height: this.pane === 'open' ? SheetSize.MEDIUM : SheetSize.LARGE, dragBar: true, showClose: true,
        keyboardAvoidMode: SheetKeyboardAvoidMode.TRANSLATE_AND_SCROLL, title: { title: this.text(this.sheetTitle()) },
        backgroundColor: this.palette.top, onDisappear: (): void => { this.openError = ''; } })
  }
}
