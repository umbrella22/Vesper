import SwiftUI

/// Dusk-gold accent (#E8A93A) used sparingly by the host shell.
let exampleShellAccent = Color(red: 232.0 / 255.0, green: 169.0 / 255.0, blue: 58.0 / 255.0)

/// Liquid Glass helpers for the host shell.
///
/// The compiler guards match the SDKs bundled with Xcode 26 and 26.1.
/// Earlier SDKs and runtimes use material and bordered button styles.
/// Glass is only applied to navigation-level and floating controls; content
/// lists keep their plain backgrounds.
extension View {
    @ViewBuilder
    func exampleGlassButtonStyle(prominent: Bool = false) -> some View {
        #if compiler(>=6.2)
        if #available(iOS 26.0, *) {
            if prominent {
                self.buttonStyle(.glassProminent).tint(exampleShellAccent)
            } else {
                self.buttonStyle(.glass)
            }
        } else {
            exampleBorderedButtonStyle(prominent: prominent)
        }
        #else
        exampleBorderedButtonStyle(prominent: prominent)
        #endif
    }

    @ViewBuilder
    private func exampleBorderedButtonStyle(prominent: Bool) -> some View {
        if prominent {
            self.buttonStyle(.borderedProminent).tint(exampleShellAccent)
        } else {
            self.buttonStyle(.bordered)
        }
    }

    @ViewBuilder
    func exampleFloatingGlass<S: Shape>(in shape: S) -> some View {
        #if compiler(>=6.2)
        if #available(iOS 26.0, *) {
            self.glassEffect(.regular.interactive(), in: shape)
        } else {
            self.background(.ultraThinMaterial, in: shape)
        }
        #else
        self.background(.ultraThinMaterial, in: shape)
        #endif
    }

    @ViewBuilder
    func exampleTabBarMinimizeOnScroll() -> some View {
        #if compiler(>=6.2)
        if #available(iOS 26.0, *) {
            self.tabBarMinimizeBehavior(.onScrollDown)
        } else {
            self
        }
        #else
        self
        #endif
    }

    @ViewBuilder
    func exampleMiniPlayerAccessory<Accessory: View>(
        isEnabled: Bool,
        @ViewBuilder content: () -> Accessory
    ) -> some View {
        #if compiler(>=6.2.1)
        if #available(iOS 26.1, *) {
            self.tabViewBottomAccessory(isEnabled: isEnabled, content: content)
        } else {
            self
        }
        #else
        self
        #endif
    }
}

/// Groups adjacent floating glass controls so they blend as one glass layer.
struct ExampleGlassGroup<Content: View>: View {
    var spacing: CGFloat = 10
    @ViewBuilder let content: () -> Content

    var body: some View {
        #if compiler(>=6.2)
        if #available(iOS 26.0, *) {
            GlassEffectContainer(spacing: spacing) {
                content()
            }
        } else {
            content()
        }
        #else
        content()
        #endif
    }
}
