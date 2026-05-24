import XCTest
@testable import VesperPlayerKit

func makeTestDashSession(sourceURL: URL) -> VesperDashSession {
    VesperDashSession(
        sourceURL: sourceURL,
        videoDecodeCapabilityProvider: testHardwareVideoDecodeCapabilityProvider
    )
}

let testHardwareVideoDecodeCapabilityProvider: VesperDashSession.VideoDecodeCapabilityProvider = { playable in
    let candidate = VesperHardwareDecodeCandidateCodec(codecName: playable.representation.codecs)
    let family = candidate.dashCodecFamily
    let supported = family != .unknown
    return VesperDashVideoDecodeCapability(
        renditionId: playable.renditionId,
        codecFamily: family,
        hardwareDecodeSupported: supported,
        decoderName: supported ? "UnitTestHardwareDecoder" : nil
    )
}

let sampleMpd = #"""
<?xml version="1.0"?>
<MPD type="static" mediaPresentationDuration="PT1M30.5S" minBufferTime="PT1.5S">
  <BaseURL>https://cdn.example.com/root/master.mpd</BaseURL>
  <Period id="p0">
    <AdaptationSet id="v" contentType="video" mimeType="video/mp4">
      <BaseURL>video/</BaseURL>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720" frameRate="30000/1001">
        <BaseURL>seg.m4s</BaseURL>
        <SegmentBase indexRange="1000-1199">
          <Initialization range="0-999"/>
        </SegmentBase>
      </Representation>
    </AdaptationSet>
    <AdaptationSet id="a" mimeType="audio/mp4" lang="ja">
      <Representation id="a1" bandwidth="128000" codecs="mp4a.40.2" audioSamplingRate="48000">
        <BaseURL>../audio/main.m4s</BaseURL>
        <SegmentBase indexRange="800-950">
          <Initialization range="0-799"/>
        </SegmentBase>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>
"""#

let sampleSegmentTemplateMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT193.680S" minBufferTime="PT5.000S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true" startWithSAP="1">
      <SegmentTemplate timescale="90000" initialization="$RepresentationID$-Header.m4s" media="$RepresentationID$-270146-i-$Number$.m4s" startNumber="1" duration="179704" presentationTimeOffset="0"/>
      <Representation id="v1_257" bandwidth="1200000" codecs="avc1.4D401E" width="768" height="432" frameRate="30000/1001"/>
    </AdaptationSet>
    <AdaptationSet mimeType="audio/mp4" segmentAlignment="true" startWithSAP="1" lang="qaa">
      <SegmentTemplate timescale="90000" initialization="$RepresentationID$-Header.m4s" media="$RepresentationID$-270146-i-$Number$.m4s" startNumber="1" duration="179704" presentationTimeOffset="0"/>
      <Representation id="v4_258" bandwidth="130800" codecs="mp4a.40.2" audioSamplingRate="48000"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

let sampleMultiVideoSegmentTemplateMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT193.680S" minBufferTime="PT5.000S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true" startWithSAP="1">
      <SegmentTemplate timescale="90000" initialization="$RepresentationID$-Header.m4s" media="$RepresentationID$-270146-i-$Number$.m4s" startNumber="1" duration="179704" presentationTimeOffset="0"/>
      <Representation id="v1_257" bandwidth="1200000" codecs="avc1.4D401E" width="768" height="432" frameRate="30000/1001"/>
      <Representation id="v2_257" bandwidth="1850000" codecs="avc1.4D401E" width="1024" height="576" frameRate="30000/1001"/>
      <Representation id="v7_257" bandwidth="5300000" codecs="avc1.4D401E" width="1920" height="1080" frameRate="30000/1001"/>
    </AdaptationSet>
    <AdaptationSet mimeType="audio/mp4" segmentAlignment="true" startWithSAP="1" lang="qaa">
      <SegmentTemplate timescale="90000" initialization="$RepresentationID$-Header.m4s" media="$RepresentationID$-270146-i-$Number$.m4s" startNumber="1" duration="179704" presentationTimeOffset="0"/>
      <Representation id="v4_258" bandwidth="130800" codecs="mp4a.40.2" audioSamplingRate="48000"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

let sampleMultiCodecSegmentTemplateMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT30S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true" startWithSAP="1">
      <SegmentTemplate timescale="1000" initialization="$RepresentationID$-init.mp4" media="$RepresentationID$-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="av1" bandwidth="760000" codecs="av01.0.05M.08" width="1280" height="720"/>
      <Representation id="hevc" bandwidth="800000" codecs="hvc1.1.6.L93.B0" width="1280" height="720"/>
      <Representation id="avc" bandwidth="800000" codecs="avc1.4D401F" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet mimeType="audio/mp4" segmentAlignment="true" startWithSAP="1" lang="und">
      <SegmentTemplate timescale="1000" initialization="$RepresentationID$-init.mp4" media="$RepresentationID$-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="audio" bandwidth="128000" codecs="mp4a.40.2" audioSamplingRate="48000"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

let sampleSegmentTimelineMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT7S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="chunk-$Time$.m4s" startNumber="7" presentationTimeOffset="5000">
        <SegmentTimeline>
          <S t="5000" d="2000" r="2"/>
          <S d="1000"/>
        </SegmentTimeline>
      </SegmentTemplate>
      <Representation id="video" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

let sampleOpenEndedSegmentTimelineMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT5.5S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init.mp4" media="chunk-$Time$.m4s">
        <SegmentTimeline>
          <S t="0" d="2000" r="-1"/>
        </SegmentTimeline>
      </SegmentTemplate>
      <Representation id="video" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

let sampleDynamicSegmentTimelineMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="dynamic" minimumUpdatePeriod="PT2S" timeShiftBufferDepth="PT20S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="chunk-$Time$.m4s" startNumber="101">
        <SegmentTimeline>
          <S t="200000" d="2000" r="2"/>
        </SegmentTimeline>
      </SegmentTemplate>
      <Representation id="live-video" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

let sampleDynamicDurationTemplateMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="dynamic" minimumUpdatePeriod="PT2S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="chunk-$Number$.m4s" startNumber="101" duration="2000"/>
      <Representation id="live-video" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

let sampleWebVttSubtitleMpd = #"""
<?xml version="1.0" encoding="UTF-8"?>
<MPD type="static" mediaPresentationDuration="PT6S" minBufferTime="PT2S">
  <Period id="period0">
    <AdaptationSet mimeType="video/mp4" segmentAlignment="true">
      <SegmentTemplate timescale="1000" initialization="init-$RepresentationID$.mp4" media="video-$Number$.m4s" startNumber="1" duration="2000"/>
      <Representation id="v1" bandwidth="800000" codecs="avc1.64001f" width="1280" height="720"/>
    </AdaptationSet>
    <AdaptationSet id="subs" contentType="text" mimeType="text/vtt" lang="en">
      <SegmentTemplate timescale="1000" media="sub-$Number$.vtt" startNumber="1" duration="2000"/>
      <Representation id="sub-en" bandwidth="1200" codecs="wvtt"/>
    </AdaptationSet>
  </Period>
</MPD>
"""#

func sidxPayloadV0() -> Data {
    var payload = Data()
    payload.append(contentsOf: [0, 0, 0, 0])
    appendUInt32(1, to: &payload)
    appendUInt32(1_000, to: &payload)
    appendUInt32(500, to: &payload)
    appendUInt32(10, to: &payload)
    appendUInt16(0, to: &payload)
    appendUInt16(2, to: &payload)
    appendReference(size: 100, duration: 2_000, startsWithSap: true, sapType: 1, sapDeltaTime: 0, to: &payload)
    appendReference(size: 150, duration: 3_000, startsWithSap: true, sapType: 2, sapDeltaTime: 5, to: &payload)
    return payload
}

func appendReference(
    size: UInt32,
    duration: UInt32,
    startsWithSap: Bool,
    sapType: UInt8,
    sapDeltaTime: UInt32,
    to data: inout Data
) {
    appendUInt32(size & 0x7fff_ffff, to: &data)
    appendUInt32(duration, to: &data)
    let sap = (UInt32(startsWithSap ? 1 : 0) << 31)
        | ((UInt32(sapType) & 0x07) << 28)
        | (sapDeltaTime & 0x0fff_ffff)
    appendUInt32(sap, to: &data)
}

func mp4Box(type: String, payload: Data) -> Data {
    var data = Data()
    appendUInt32(UInt32(payload.count + 8), to: &data)
    data.append(contentsOf: type.utf8)
    data.append(payload)
    return data
}

func appendUInt16(_ value: UInt16, to data: inout Data) {
    data.append(UInt8((value >> 8) & 0xff))
    data.append(UInt8(value & 0xff))
}

func appendUInt32(_ value: UInt32, to data: inout Data) {
    data.append(UInt8((value >> 24) & 0xff))
    data.append(UInt8((value >> 16) & 0xff))
    data.append(UInt8((value >> 8) & 0xff))
    data.append(UInt8(value & 0xff))
}

func countOccurrences(of needle: String, in haystack: String) -> Int {
    haystack.components(separatedBy: needle).count - 1
}

func firstMatch(_ pattern: String, in text: String) -> String? {
    text.range(of: pattern, options: .regularExpression).map { String(text[$0]) }
}

func firstResourceLoaderSegmentSessionId(in playlist: String) throws -> String {
    let urlText = try XCTUnwrap(
        firstMatch(#"vesper-dash://segment/[^"]+"#, in: playlist)
    )
    let url = try XCTUnwrap(URL(string: urlText))
    let components = url.pathComponents.filter { $0 != "/" }
    return try XCTUnwrap(components.first)
}

func eventAttributes(
    _ name: String,
    in events: [(name: String, attributes: [String: String])],
    where matches: ([String: String]) -> Bool = { _ in true }
) -> [String: String]? {
    events.first { $0.name == name && matches($0.attributes) }?.attributes
}

final class CountingDashNetworkClient: VesperDashNetworkClient {
    private let dataByURL: [URL: Data]
    private let delayNanoseconds: UInt64
    private let lock = NSLock()
    private var counts: [String: Int] = [:]

    init(dataByURL: [URL: Data], delayNanoseconds: UInt64 = 0) {
        self.dataByURL = dataByURL
        self.delayNanoseconds = delayNanoseconds
        super.init()
    }

    override func data(
        for url: URL,
        byteRange: VesperDashByteRange? = nil
    ) async throws -> Data {
        if delayNanoseconds > 0 {
            try await Task.sleep(nanoseconds: delayNanoseconds)
        }
        incrementRequestCount(for: url, byteRange: byteRange)
        guard let payload = dataByURL[url] else {
            throw VesperDashBridgeError.network("missing test payload for \(url.absoluteString)")
        }
        guard let byteRange else {
            return payload
        }
        let start = Int(byteRange.start)
        let end = Int(byteRange.end)
        guard start >= 0, end < payload.count, start <= end else {
            throw VesperDashBridgeError.network("test byte range is out of bounds")
        }
        return payload.subdata(in: start..<(end + 1))
    }

    override func download(
        for url: URL,
        byteRange: VesperDashByteRange? = nil,
        to destinationURL: URL
    ) async throws -> UInt64 {
        let payload = try await data(for: url, byteRange: byteRange)
        try FileManager.default.createDirectory(
            at: destinationURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try payload.write(to: destinationURL)
        return UInt64(payload.count)
    }

    func requestCount(for url: URL, byteRange: VesperDashByteRange? = nil) -> Int {
        lock.lock()
        defer { lock.unlock() }
        return counts[requestKey(for: url, byteRange: byteRange), default: 0]
    }

    private func incrementRequestCount(for url: URL, byteRange: VesperDashByteRange?) {
        lock.lock()
        defer { lock.unlock() }
        counts[requestKey(for: url, byteRange: byteRange), default: 0] += 1
    }

    private func requestKey(for url: URL, byteRange: VesperDashByteRange?) -> String {
        if let byteRange {
            return "\(url.absoluteString)#\(byteRange.start)-\(byteRange.end)"
        }
        return url.absoluteString
    }
}

func sampleSegmentBaseMediaData() -> Data {
    var payload = mp4Box(type: "ftyp", payload: Data(repeating: 0, count: 992))
    let sidxBox = mp4Box(type: "sidx", payload: sidxPayloadV0())
    payload.append(sidxBox)
    if payload.count < 1_600 {
        payload.append(Data(repeating: 0x55, count: 1_600 - payload.count))
    }
    return payload
}

func writeSegmentTemplateFiles(
    directory: URL,
    renditionId: String,
    initData: Data,
    mediaData: Data,
    segmentCount: Int = 97
) throws {
    try initData.write(to: directory.appendingPathComponent("\(renditionId)-Header.m4s"))
    for number in 1...segmentCount {
        try mediaData.write(
            to: directory.appendingPathComponent("\(renditionId)-270146-i-\(number).m4s")
        )
    }
}
