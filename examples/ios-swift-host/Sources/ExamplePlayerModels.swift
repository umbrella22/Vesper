import Foundation
import SwiftUI
import VesperPlayerKit

enum ExamplePlayerSheet: String, Identifiable {
    case menu
    case quality
    case audio
    case subtitle
    case speed

    var id: String { rawValue }
}

enum ExampleThemeMode: String, CaseIterable, Identifiable {
    case system
    case light
    case dark

    var id: String { rawValue }

    var preferredColorScheme: ColorScheme? {
        switch self {
        case .system:
            nil
        case .light:
            .light
        case .dark:
            .dark
        }
    }

    var title: String {
        switch self {
        case .system:
            "System"
        case .light:
            "Light"
        case .dark:
            "Dark"
        }
    }

    var systemImage: String {
        switch self {
        case .system:
            "circle.lefthalf.filled"
        case .light:
            "sun.max.fill"
        case .dark:
            "moon.fill"
        }
    }
}

struct ExampleHostPalette {
    let pageTop: Color
    let pageBottom: Color
    let sectionBackground: Color
    let sectionStroke: Color
    let title: Color
    let body: Color
    let fieldBackground: Color
    let fieldText: Color
    let primaryAction: Color
}

func exampleHostPalette(useDarkTheme: Bool) -> ExampleHostPalette {
    if useDarkTheme {
        ExampleHostPalette(
            pageTop: Color(red: 0.047, green: 0.063, blue: 0.098),
            pageBottom: Color(red: 0.023, green: 0.027, blue: 0.043),
            sectionBackground: .white.opacity(0.04),
            sectionStroke: .white.opacity(0.06),
            title: .white,
            body: .white.opacity(0.62),
            fieldBackground: .white.opacity(0.06),
            fieldText: .white,
            primaryAction: Color(red: 0.165, green: 0.545, blue: 1.0)
        )
    } else {
        ExampleHostPalette(
            pageTop: Color(red: 0.972, green: 0.949, blue: 0.918),
            pageBottom: Color(red: 0.949, green: 0.957, blue: 0.976),
            sectionBackground: .white.opacity(0.88),
            sectionStroke: Color.black.opacity(0.06),
            title: Color(red: 0.063, green: 0.082, blue: 0.129),
            body: Color(red: 0.361, green: 0.400, blue: 0.478),
            fieldBackground: Color(red: 0.965, green: 0.969, blue: 0.980),
            fieldText: Color(red: 0.063, green: 0.082, blue: 0.129),
            primaryAction: Color(red: 0.145, green: 0.427, blue: 1.0)
        )
    }
}

struct AbrPreset: Identifiable {
    let id: String
    let title: String
    let subtitle: String
    let policy: VesperAbrPolicy
}

let IOS_HLS_DEMO_URL =
    "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8"

func iosHlsDemoSource() -> VesperPlayerSource {
    VesperPlayerSource.hls(
        url: URL(string: IOS_HLS_DEMO_URL)!,
        label: "HLS Demo (BipBop)"
    )
}
