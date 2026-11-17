/** Internal bridge. Product APIs are exported by the ArkTS Host Kit. */
declare const bridge: {
  create(configurationJson: string): string;
  call(handle: string, commandJson: string): string;
  release(handle: string): void;
  queryVideoDecoder(mimeType: string, width: number, height: number, profile: number, level: number): Object;
};
export default bridge;
