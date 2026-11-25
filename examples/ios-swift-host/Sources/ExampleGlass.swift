import SwiftUI

/// Dusk-gold accent (#E8A93A) used sparingly by the host shell.
let exampleShellAccent = Color(red: 232.0 / 255.0, green: 169.0 / 255.0, blue: 58.0 / 255.0)

/// Liquid Glass helpers for the host shell.
///
/// On iOS 26+ these map to the system glass APIs. On iOS 17 through 25 they
/// fall back to `.ultraThinMaterial` and the regular bordered button styles.
/// Glass is only applied to navigation-level and floating controls; content
/// lists keep their plain backgrounds.
extension View {
    @ViewBuilder
    func exampleGlassButtonStyle(prominent: Bool = false) -> some View {
        if #available(iOS 26.0, *) {
            if prominent {
                self.buttonStyle(.glassProminent).tint(exampleShellAccent)
            } else {
                self.buttonStyle(.glass)
            }
        } else {
            if prominent {
                self.buttonStyle(.borderedProminent).tint(exampleShellAccent)
            } else {
                self.buttonStyle(.bordered)
            }
        }
    }

    @ViewBuilder
    func exampleFloatingGlass<S: Shape>(in shape: S) -> some View {
        if #available(iOS 26.0, *) {
            self.glassEffect(.regular.interactive(), in: shape)
        } else {
            self.background(.ultraThinMaterial, in: shape)
        }
    }

    @ViewBuilder
    func exampleTabBarMinimizeOnScroll() -> some View {
        if #available(iOS 26.0, *) {
            self.tabBarMinimizeBehavior(.onScrollDown)
        } else {
            self
        }
    }

    @ViewBuilder
    func exampleMiniPlayerAccessory<Accessory: View>(
        isEnabled: Bool,
        @ViewBuilder content: () -> Accessory
    ) -> some View {
        if #available(iOS 26.1, *) {
            self.tabViewBottomAccessory(isEnabled: isEnabled, content: content)
        } else {
            self
        }
    }
}

/// Groups adjacent floating glass controls so they blend as one glass layer.
struct ExampleGlassGroup<Content: View>: View {
    var spacing: CGFloat = 10
    @ViewBuilder let content: () -> Content

    var body: some View {
        if #available(iOS 26.0, *) {
            GlassEffectContainer(spacing: spacing) {
                content()
            }
        } else {
            content()
        }
    }
}
