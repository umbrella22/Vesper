import Foundation
import XCTest
@testable import VesperPlayerKit

final class VesperFairPlayDrmCoordinatorTests: XCTestCase {
    func testCertificateBase64TakesPrecedenceOverUri() async throws {
        let loader = VesperFairPlayCertificateLoader(
            dataLoader: MockFairPlayDataLoader(data: Data("uri-certificate".utf8))
        )
        let configuration = VesperPlayerDrmConfiguration(
            keySystem: "fairPlay",
            licenseUri: "https://license.example.com/fairplay",
            fairPlayCertificateUri: "https://cert.example.com/fairplay.cer",
            fairPlayCertificateBase64: Data("base64-certificate".utf8).base64EncodedString()
        )

        let data = try await loader.certificateData(for: configuration)

        XCTAssertEqual(data, Data("base64-certificate".utf8))
    }

    func testCertificateLoaderReportsMissingCertificate() async {
        let loader = VesperFairPlayCertificateLoader(
            dataLoader: MockFairPlayDataLoader(data: Data())
        )
        let configuration = VesperPlayerDrmConfiguration(
            keySystem: "fairPlay",
            licenseUri: "https://license.example.com/fairplay"
        )

        do {
            _ = try await loader.certificateData(for: configuration)
            XCTFail("Expected missing FairPlay certificate error.")
        } catch let error as VesperPlayerDrmRuntimeError {
            XCTAssertEqual(error.reason, "fairPlayCertificateMissing")
            XCTAssertEqual(error.details["route"], "direct")
            XCTAssertEqual(error.details["keySystem"], "fairPlay")
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    func testCertificateUriFailureReportsTypedDrmError() async {
        let loader = VesperFairPlayCertificateLoader(
            dataLoader: MockFairPlayDataLoader(statusCode: 404, data: Data("missing".utf8))
        )
        let configuration = VesperPlayerDrmConfiguration(
            keySystem: "fairPlay",
            licenseUri: "https://license.example.com/fairplay",
            fairPlayCertificateUri: "https://cert.example.com/fairplay.cer"
        )

        do {
            _ = try await loader.certificateData(for: configuration)
            XCTFail("Expected FairPlay certificate request failure.")
        } catch let error as VesperPlayerDrmRuntimeError {
            XCTAssertEqual(error.reason, "fairPlayCertificateRequestFailed")
            XCTAssertEqual(error.retriable, true)
            XCTAssertEqual(error.details["route"], "direct")
            XCTAssertEqual(error.details["keySystem"], "fairPlay")
            XCTAssertEqual(error.details["certificateUriHost"], "cert.example.com")
            XCTAssertEqual(error.details["httpStatusCode"], "404")
            XCTAssertNil(error.details["fairPlayCertificateUri"])
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    func testCertificateNetworkFailureReportsRetriableHostOnlyDetails() async {
        let loader = VesperFairPlayCertificateLoader(
            dataLoader: MockFairPlayDataLoader(
                error: URLError(.networkConnectionLost)
            )
        )
        let configuration = VesperPlayerDrmConfiguration(
            keySystem: "fairPlay",
            licenseUri: "https://license.example.com/fairplay",
            fairPlayCertificateUri: "https://cert.example.com/fairplay.cer"
        )

        do {
            _ = try await loader.certificateData(for: configuration)
            XCTFail("Expected FairPlay certificate network failure.")
        } catch let error as VesperPlayerDrmRuntimeError {
            XCTAssertEqual(error.reason, "fairPlayCertificateRequestFailed")
            XCTAssertEqual(error.retriable, true)
            XCTAssertEqual(error.details["certificateUriHost"], "cert.example.com")
            XCTAssertEqual(error.details["errorClass"], "URLError")
            XCTAssertNotNil(error.details["errorMessage"])
            XCTAssertNil(error.details["fairPlayCertificateUri"])
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    func testLicenseRequestUsesLicenseHeadersOnly() throws {
        let requester = VesperFairPlayLicenseRequester(
            dataLoader: MockFairPlayDataLoader(data: Data("ckc".utf8))
        )
        let source = VesperPlayerSource.hls(
            url: URL(string: "https://media.example.com/drm.m3u8")!,
            label: "FairPlay",
            headers: ["X-Media": "media"],
            drmConfiguration: VesperPlayerDrmConfiguration(
                keySystem: "fairPlay",
                licenseUri: "https://license.example.com/fairplay",
                licenseHeaders: ["Authorization": "Bearer drm"]
            )
        )

        let request = try requester.makeLicenseRequest(
            spcData: Data("spc".utf8),
            drmConfiguration: try XCTUnwrap(source.drmConfiguration)
        )

        XCTAssertEqual(request.url?.absoluteString, "https://license.example.com/fairplay")
        XCTAssertEqual(request.httpMethod, "POST")
        XCTAssertEqual(request.httpBody, Data("spc".utf8))
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer drm")
        XCTAssertNil(request.value(forHTTPHeaderField: "X-Media"))
        XCTAssertEqual(
            request.value(forHTTPHeaderField: "Content-Type"),
            "application/octet-stream"
        )
    }

    func testLicenseFailureReportsTypedDrmError() async {
        let requester = VesperFairPlayLicenseRequester(
            dataLoader: MockFairPlayDataLoader(statusCode: 403, data: Data("denied".utf8))
        )
        let configuration = VesperPlayerDrmConfiguration(
            keySystem: "fairPlay",
            licenseUri: "https://license.example.com/fairplay"
        )

        do {
            _ = try await requester.ckcData(
                spcData: Data("spc".utf8),
                drmConfiguration: configuration
            )
            XCTFail("Expected FairPlay license failure.")
        } catch let error as VesperPlayerDrmRuntimeError {
            XCTAssertEqual(error.reason, "fairPlayLicenseRequestFailed")
            XCTAssertEqual(error.retriable, true)
            XCTAssertEqual(error.details["route"], "direct")
            XCTAssertEqual(error.details["keySystem"], "fairPlay")
            XCTAssertEqual(error.details["licenseUriHost"], "license.example.com")
            XCTAssertEqual(error.details["httpStatusCode"], "403")
            XCTAssertNil(error.details["licenseUri"])
            XCTAssertNil(error.details["Authorization"])
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    func testBlankLicenseUriReportsNonRetriableConfigurationError() throws {
        let requester = VesperFairPlayLicenseRequester(
            dataLoader: MockFairPlayDataLoader(data: Data("ckc".utf8))
        )
        let configuration = VesperPlayerDrmConfiguration(
            keySystem: "fairPlay",
            licenseUri: " ",
            fairPlayCertificateBase64: Data("certificate".utf8).base64EncodedString()
        )

        XCTAssertThrowsError(
            try requester.makeLicenseRequest(
                spcData: Data("spc".utf8),
                drmConfiguration: configuration
            )
        ) { error in
            let drmError = error as? VesperPlayerDrmRuntimeError
            XCTAssertEqual(drmError?.reason, "fairPlayLicenseUriInvalid")
            XCTAssertEqual(drmError?.retriable, false)
        }
    }

    func testContentIdentifierPreservesFullSkdUri() {
        let data = fairPlayContentIdentifierData(
            from: "skd://asset-123/path/to/key?variant=main",
            fallback: "https://media.example.com/drm.m3u8"
        )

        XCTAssertEqual(
            String(decoding: data, as: UTF8.self),
            "skd://asset-123/path/to/key?variant=main"
        )
    }
}

private struct MockFairPlayDataLoader: VesperFairPlayDataLoading {
    let statusCode: Int
    let data: Data
    let error: Error?

    init(statusCode: Int = 200, data: Data = Data(), error: Error? = nil) {
        self.statusCode = statusCode
        self.data = data
        self.error = error
    }

    func data(for request: URLRequest) async throws -> (Data, URLResponse) {
        if let error {
            throw error
        }
        let response = HTTPURLResponse(
            url: request.url ?? URL(string: "https://example.com")!,
            statusCode: statusCode,
            httpVersion: nil,
            headerFields: nil
        )!
        return (data, response)
    }
}
