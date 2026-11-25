/** Host-shell navigation rules shared by the page and its contract tests. */
export const EXAMPLE_TABS: string[] = ['play', 'library', 'downloads', 'lab'];
/** Every half-modal sheet the page presents: play-tab sources, queue, and tools; title-bar settings; new download. */
export const EXAMPLE_SHEETS: string[] = ['open', 'queue', 'tools', 'settings', 'download'];
export const EXAMPLE_COLLECTIONS: string[] = ['samples', 'custom'];

export function tabIndex(tab: string): number {
  const index = EXAMPLE_TABS.indexOf(tab);
  return index < 0 ? 0 : index;
}

export function tabAt(index: number): string {
  return Number.isInteger(index) && index >= 0 && index < EXAMPLE_TABS.length ? EXAMPLE_TABS[index] : 'play';
}

export function isSheet(name: string): boolean { return EXAMPLE_SHEETS.includes(name); }

/** Unknown names never present a sheet, so a binding cannot show an empty or untitled sheet. */
export function presentSheet(name: string): string { return isSheet(name) ? name : ''; }

/** A sheet closes after its action succeeds and stays open, showing the error, when it fails. */
export function sheetAfterAction(sheet: string, succeeded: boolean): string { return succeeded || !isSheet(sheet) ? '' : sheet; }

/** Wide windows switch to a side rail and a split play page; large fonts raise the threshold. */
export function isWideLayout(width: number, fontScale: number): boolean {
  return width >= Math.max(840, 680 * Math.min(1.5, Math.max(1, fontScale)));
}

/** Mirrors the prototype's clamp(320px, 30vw, 400px) right column. */
export function sidebarWidth(width: number): number { return Math.min(400, Math.max(320, width * 0.3)); }

export interface ShellBackState {
  sheet: string;
  routes: number;
  settingsPane?: string;
  fullscreen: boolean;
  tab: string;
}

/** Back closes the innermost surface first and leaves the app only from the play tab. */
export function shellBackAction(state: ShellBackState): string {
  if (state.sheet.length > 0) { return 'closeSheet'; }
  if (state.settingsPane !== undefined) { return 'stagePane'; }
  if (state.fullscreen) { return 'exitFullscreen'; }
  if (state.routes > 0) { return 'popRoute'; }
  if (state.tab !== 'play') { return 'homeTab'; }
  return 'system';
}

export function parentStagePane(pane: string): string | undefined {
  return pane === 'menu' ? undefined : pane === 'captionStyle' ? 'subtitle' : 'menu';
}

export interface ExampleCollectionMember { builtin: boolean; }

/** Built-in samples and user-added media form the two library collections. */
export function collectionMembers<T extends ExampleCollectionMember>(items: T[], collection: string): T[] {
  if (collection === 'samples') { return items.filter((item: T): boolean => item.builtin); }
  if (collection === 'custom') { return items.filter((item: T): boolean => !item.builtin); }
  return [];
}
