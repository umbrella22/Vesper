import { UIAbility, Configuration, ConfigurationConstant } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';

export default class EntryAbility extends UIAbility {
  onWindowStageCreate(windowStage: window.WindowStage): void {
    const mode = this.context.config.colorMode ?? ConfigurationConstant.ColorMode.COLOR_MODE_LIGHT;
    AppStorage.setOrCreate('vesper.example.colorMode', mode);
    AppStorage.setOrCreate('vesper.example.fontScale', this.context.config.fontSizeScale ?? 1);
    AppStorage.setOrCreate('vesper.example.foreground', true);
    windowStage.loadContent('pages/Index');
  }
  onConfigurationUpdate(configuration: Configuration): void {
    if (configuration.colorMode !== undefined) { AppStorage.setOrCreate('vesper.example.colorMode', configuration.colorMode); }
    if (configuration.fontSizeScale !== undefined) { AppStorage.setOrCreate('vesper.example.fontScale', configuration.fontSizeScale); }
  }
  onForeground(): void { AppStorage.setOrCreate('vesper.example.foreground', true); }
  onBackground(): void { AppStorage.setOrCreate('vesper.example.foreground', false); }
}
