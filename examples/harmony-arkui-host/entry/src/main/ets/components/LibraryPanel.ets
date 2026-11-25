import { ExampleModel, ExampleQueueItem } from '../model/ExampleModel';
import { examplePalette, ExamplePalette } from '../model/ExampleSupport';
import { collectionMembers, EXAMPLE_COLLECTIONS } from '../model/ExampleShell';
import { ExampleHeading } from './ExampleControls';

/** Library root: one row per collection; the page pushes the collection destination. */
@Component
export struct LibraryPanel {
  @ObjectLink model: ExampleModel;
  @Prop palette: ExamplePalette = examplePalette(false);
  onOpenCollection: (collection: string) => void = (): void => {};
  private text(key: string): string { return this.model.text('example_' + key); }
  build() {
    Column({ space: 16 }) {
      ExampleHeading({ title: this.text('library'), detail: this.text('library_detail'), palette: this.palette })
      List() {
        ForEach(EXAMPLE_COLLECTIONS, (collection: string) => {
          ListItem() {
            Row({ space: 12 }) {
              SymbolGlyph(collection === 'samples' ? $r('sys.symbol.film') : $r('sys.symbol.folder')).fontSize(22)
                .fontColor([this.palette.title]).width(40).height(40).backgroundColor(this.palette.field).borderRadius(10).accessibilityLevel('no')
              Column({ space: 4 }) {
                Text(this.text('collection_' + collection)).fontSize(16).fontWeight(FontWeight.Medium).fontColor(this.palette.title)
                Text(this.text('collection_' + collection + '_detail')).fontSize(12).fontColor(this.palette.secondary).maxLines(2)
              }.layoutWeight(1).alignItems(HorizontalAlign.Start)
              Text(String(collectionMembers(this.model.queue, collection).length)).fontSize(13).fontColor(this.palette.secondary)
              SymbolGlyph($r('sys.symbol.chevron_right')).fontSize(16).fontColor([this.palette.secondary]).accessibilityLevel('no')
            }.padding({ left: 16, right: 16, top: 12, bottom: 12 }).width('100%').constraintSize({ minHeight: 64 })
          }.onClick((): void => { this.onOpenCollection(collection); })
            .accessibilityText(this.text('collection_' + collection))
        }, (collection: string): string => collection)
      }.width('100%').divider({ strokeWidth: 0.5, color: this.palette.line, startMargin: 68 })
        .backgroundColor(this.palette.surface).borderRadius(16)
    }.padding(20).constraintSize({ maxWidth: 860 }).width('100%').alignItems(HorizontalAlign.Start)
  }
}

/** Collection second-level page: tapping an item plays it and returns to the play tab. */
@Component
export struct CollectionPanel {
  @ObjectLink model: ExampleModel;
  @Prop palette: ExamplePalette = examplePalette(false);
  @Prop collection: string = 'samples';
  onPlayed: () => void = (): void => {};
  private available(): boolean { return this.model.ready && !this.model.busy; }
  build() {
    Column({ space: 12 }) {
      Text(this.model.text('example_collection_' + this.collection + '_detail')).fontSize(13).fontColor(this.palette.secondary).width('100%')
      if (collectionMembers(this.model.queue, this.collection).length === 0) {
        Text(this.model.text('example_collection_empty')).fontSize(14).fontColor(this.palette.secondary)
          .width('100%').padding({ top: 32, bottom: 32 }).textAlign(TextAlign.Center)
      }
      List() {
        ForEach(collectionMembers(this.model.queue, this.collection), (item: ExampleQueueItem) => {
          ListItem() {
            Row({ space: 12 }) {
              Column({ space: 4 }) {
                Text(item.title).fontSize(15).fontWeight(FontWeight.Medium).fontColor(this.palette.title).maxLines(2)
                Text(item.id === this.model.selectedId ? this.model.text('example_now_playing') : item.detail)
                  .fontSize(12).fontColor(item.id === this.model.selectedId ? this.palette.accent : this.palette.secondary).maxLines(2)
              }.layoutWeight(1).alignItems(HorizontalAlign.Start)
              SymbolGlyph(item.id === this.model.selectedId ? $r('sys.symbol.waveform') : $r('sys.symbol.play_fill'))
                .fontSize(16).fontColor([this.palette.secondary]).accessibilityLevel('no')
            }.padding({ left: 16, right: 16, top: 12, bottom: 12 }).width('100%').constraintSize({ minHeight: 64 })
          }.enabled(this.available()).accessibilityText(item.title).accessibilitySelected(item.id === this.model.selectedId)
            .onClick((): void => { this.model.play(item); this.onPlayed(); })
        }, (item: ExampleQueueItem): string => item.id)
      }.width('100%').divider({ strokeWidth: 0.5, color: this.palette.line, startMargin: 16 })
        .backgroundColor(this.palette.surface).borderRadius(16)
    }.padding(20).constraintSize({ maxWidth: 860 }).width('100%').alignItems(HorizontalAlign.Start)
  }
}
