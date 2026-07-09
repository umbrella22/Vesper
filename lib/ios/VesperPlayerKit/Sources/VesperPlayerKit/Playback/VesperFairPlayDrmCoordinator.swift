@preconcurrency import AVFoundation
import Foundation

protocol VesperFairPlayDataLoading: Sendable {
    func data(for request: URLRequest) async throws -> (Data, URLResponse)
}

struct VesperFairPlayURLSessionDataLoader: VesperFairPlayDataLoading {
    func data(for request: URLRequest) async throws -> (Data, URLResponse) {
        try await URLSession.shared.data(for: request)
    }
}

struct VesperFairPlayCertificateLoader: Sendable {
    let dataLoader: any VesperFairPlayDataLoading

    init(dataLoader: any VesperFairPlayDataLoading = VesperFairPlayURLSessionDataLoader()) {
        self.dataLoader = dataLoader
    }

    func certificateData(for drmConfiguration: VesperPlayerDrmConfiguration) async throws -> Data {
        if let base64 = drmConfiguration.fairPlayCertificateBase64?.vesperTrimmedNonEmpty {
            guard let data = Data(base64Encoded: base64) else {
                throw VesperPlayerDrmRuntimeError(
                    reason: "fairPlayCertificateInvalid",
                    route: "direct",
                    keySystem: drmConfiguration.keySystem,
                    message: "FairPlay certificate base64 data is invalid.",
                    details: ["errorClass": "DataBase64Decoding"]
                )
            }
            return data
        }

        guard let uri = drmConfiguration.fairPlayCertificateUri?.vesperTrimmedNonEmpty,
              let url = URL(string: uri),
              url.scheme?.vesperTrimmedNonEmpty != nil
        else {
            throw VesperPlayerDrmRuntimeError(
                reason: "fairPlayCertificateMissing",
                route: "direct",
                keySystem: drmConfiguration.keySystem,
                message: "FairPlay requires a certificate URI or base64 certificate data."
            )
        }

        do {
            let (data, response) = try await dataLoader.data(for: URLRequest(url: url))
            try validateFairPlayHTTPResponse(
                response,
                reason: "fairPlayCertificateRequestFailed",
                route: "direct",
                keySystem: drmConfiguration.keySystem,
                details: fairPlayUriHostDetails(key: "certificateUriHost", url: url)
            )
            return data
        } catch let drmError as VesperPlayerDrmRuntimeError {
            throw drmError
        } catch {
            throw VesperPlayerDrmRuntimeError(
                reason: "fairPlayCertificateRequestFailed",
                route: "direct",
                keySystem: drmConfiguration.keySystem,
                message: error.localizedDescription,
                retriable: true,
                details: fairPlayErrorDetails(
                    from: error,
                    merging: fairPlayUriHostDetails(key: "certificateUriHost", url: url)
                )
            )
        }
    }
}

struct VesperFairPlayLicenseRequester: Sendable {
    let dataLoader: any VesperFairPlayDataLoading

    init(dataLoader: any VesperFairPlayDataLoading = VesperFairPlayURLSessionDataLoader()) {
        self.dataLoader = dataLoader
    }

    func makeLicenseRequest(
        spcData: Data,
        drmConfiguration: VesperPlayerDrmConfiguration
    ) throws -> URLRequest {
        guard let uri = drmConfiguration.licenseUri.vesperTrimmedNonEmpty,
              let url = URL(string: uri),
              url.scheme?.vesperTrimmedNonEmpty != nil
        else {
            throw VesperPlayerDrmRuntimeError(
                reason: "fairPlayLicenseUriInvalid",
                route: "direct",
                keySystem: drmConfiguration.keySystem,
                message: "FairPlay license URI must be a non-empty absolute URL."
            )
        }

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.httpBody = spcData
        for (name, value) in drmConfiguration.licenseHeaders {
            request.setValue(value, forHTTPHeaderField: name)
        }
        if request.value(forHTTPHeaderField: "Content-Type") == nil {
            request.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
        }
        return request
    }

    func ckcData(
        spcData: Data,
        drmConfiguration: VesperPlayerDrmConfiguration
    ) async throws -> Data {
        let request = try makeLicenseRequest(
            spcData: spcData,
            drmConfiguration: drmConfiguration
        )
        do {
            let (data, response) = try await dataLoader.data(for: request)
            try validateFairPlayHTTPResponse(
                response,
                reason: "fairPlayLicenseRequestFailed",
                route: "direct",
                keySystem: drmConfiguration.keySystem,
                details: fairPlayUriHostDetails(key: "licenseUriHost", url: request.url)
            )
            return data
        } catch let drmError as VesperPlayerDrmRuntimeError {
            throw drmError
        } catch {
            throw VesperPlayerDrmRuntimeError(
                reason: "fairPlayLicenseRequestFailed",
                route: "direct",
                keySystem: drmConfiguration.keySystem,
                message: error.localizedDescription,
                retriable: true,
                details: fairPlayErrorDetails(
                    from: error,
                    merging: fairPlayUriHostDetails(key: "licenseUriHost", url: request.url)
                )
            )
        }
    }
}

final class VesperFairPlayDrmCoordinator:
    NSObject,
    AVContentKeySessionDelegate,
    @unchecked Sendable
{
    let resourceLoadingQueue: DispatchQueue

    private let source: VesperPlayerSource
    private let drmConfiguration: VesperPlayerDrmConfiguration
    private let certificateLoader: VesperFairPlayCertificateLoader
    private let licenseRequester: VesperFairPlayLicenseRequester
    private let onError: @Sendable (Error) -> Void
    private let stateLock = NSLock()
    private let contentKeySession: AVContentKeySession
    private var contentKeyRecipient: AVURLAsset?
    private var pendingTasks: [ObjectIdentifier: Task<Void, Never>] = [:]
    private var closed = false

    static func make(
        source: VesperPlayerSource,
        certificateLoader: VesperFairPlayCertificateLoader = VesperFairPlayCertificateLoader(),
        licenseRequester: VesperFairPlayLicenseRequester = VesperFairPlayLicenseRequester(),
        onError: @escaping @Sendable (Error) -> Void
    ) throws -> VesperFairPlayDrmCoordinator {
#if targetEnvironment(simulator)
        throw VesperPlayerDrmRuntimeError(
            reason: "fairPlaySimulatorUnsupported",
            route: "direct",
            keySystem: source.drmConfiguration?.keySystem ?? "fairPlay",
            message: "FairPlay Streaming content key sessions are not available on iOS Simulator.",
            details: ["errorClass": "AVContentKeySessionUnavailable"]
        )
#else
        return VesperFairPlayDrmCoordinator(
            source: source,
            certificateLoader: certificateLoader,
            licenseRequester: licenseRequester,
            onError: onError
        )
#endif
    }

    private init(
        source: VesperPlayerSource,
        certificateLoader: VesperFairPlayCertificateLoader,
        licenseRequester: VesperFairPlayLicenseRequester,
        onError: @escaping @Sendable (Error) -> Void
    ) {
        self.source = source
        drmConfiguration = source.drmConfiguration ?? VesperPlayerDrmConfiguration(
            keySystem: "fairPlay",
            licenseUri: ""
        )
        self.certificateLoader = certificateLoader
        self.licenseRequester = licenseRequester
        self.onError = onError
        resourceLoadingQueue = DispatchQueue(
            label: "io.github.ikaros.vesper.player.fairplay.\(UUID().uuidString)"
        )
        contentKeySession = AVContentKeySession(keySystem: .fairPlayStreaming)
        super.init()
        contentKeySession.setDelegate(self, queue: resourceLoadingQueue)
    }

    func attach(to asset: AVURLAsset) {
        stateLock.lock()
        let shouldAttach = !closed
        if shouldAttach {
            contentKeyRecipient = asset
        }
        stateLock.unlock()

        guard shouldAttach else { return }
        contentKeySession.addContentKeyRecipient(asset)
    }

    func cancelPendingRequests() {
        let tasks = takePendingTasks()
        tasks.forEach { $0.cancel() }
    }

    func close() {
        let tasks: [Task<Void, Never>]
        let recipient: AVURLAsset?
        stateLock.lock()
        guard !closed else {
            stateLock.unlock()
            return
        }
        closed = true
        tasks = Array(pendingTasks.values)
        pendingTasks.removeAll()
        recipient = contentKeyRecipient
        contentKeyRecipient = nil
        stateLock.unlock()

        tasks.forEach { $0.cancel() }
        if let recipient {
            contentKeySession.removeContentKeyRecipient(recipient)
        }
        contentKeySession.setDelegate(nil, queue: nil)
        contentKeySession.expire()
    }

    var isClosedForTesting: Bool {
        stateLock.lock()
        defer { stateLock.unlock() }
        return closed
    }

    func contentKeySession(
        _ session: AVContentKeySession,
        didProvide keyRequest: AVContentKeyRequest
    ) {
        handleContentKeyRequest(keyRequest)
    }

    func contentKeySession(
        _ session: AVContentKeySession,
        didProvideRenewing keyRequest: AVContentKeyRequest
    ) {
        handleContentKeyRequest(keyRequest)
    }

    func contentKeySession(
        _ session: AVContentKeySession,
        contentKeyRequest keyRequest: AVContentKeyRequest,
        didFailWithError err: Error
    ) {
        report(
            VesperPlayerDrmRuntimeError(
                reason: "fairPlayContentKeyRequestFailed",
                route: "direct",
                keySystem: drmConfiguration.keySystem,
                message: err.localizedDescription,
                retriable: true,
                details: fairPlayErrorDetails(from: err)
            )
        )
    }

    func contentKeySession(
        _ session: AVContentKeySession,
        shouldRetry keyRequest: AVContentKeyRequest,
        reason retryReason: AVContentKeyRequest.RetryReason
    ) -> Bool {
        false
    }

    private func handleContentKeyRequest(_ keyRequest: AVContentKeyRequest) {
        let requestId = ObjectIdentifier(keyRequest)
        let task = Task { [weak self, keyRequest] in
            guard let self else { return }
            do {
                let certificate = try await certificateLoader.certificateData(
                    for: drmConfiguration
                )
                let contentIdentifier = fairPlayContentIdentifierData(
                    from: keyRequest.identifier,
                    fallback: source.uri
                )
                let spcData = try await streamingContentKeyRequestData(
                    for: keyRequest,
                    certificate: certificate,
                    contentIdentifier: contentIdentifier
                )
                let ckcData = try await licenseRequester.ckcData(
                    spcData: spcData,
                    drmConfiguration: drmConfiguration
                )
                try Task.checkCancellation()
                keyRequest.processContentKeyResponse(
                    AVContentKeyResponse(fairPlayStreamingKeyResponseData: ckcData)
                )
                removePendingTask(requestId)
            } catch is CancellationError {
                // Mark the key request as terminated so AVContentKeySession does
                // not leave it hanging. `cancelPendingRequests()` itself only
                // cancels the Task; without an explicit terminal signal the
                // session would keep the request pending and could block later
                // key reuse for the same initialization data.
                keyRequest.processContentKeyResponseError(CancellationError() as NSError)
                removePendingTask(requestId)
                return
            } catch {
                let drmError = normalizeFairPlayError(error, fallbackReason: "fairPlayCkcRequestFailed")
                keyRequest.processContentKeyResponseError(drmError as NSError)
                removePendingTask(requestId)
                report(drmError)
            }
        }
        stateLock.lock()
        if closed {
            stateLock.unlock()
            task.cancel()
            return
        }
        pendingTasks[requestId] = task
        stateLock.unlock()
    }

    private func streamingContentKeyRequestData(
        for keyRequest: AVContentKeyRequest,
        certificate: Data,
        contentIdentifier: Data
    ) async throws -> Data {
        try await withCheckedThrowingContinuation { continuation in
            keyRequest.makeStreamingContentKeyRequestData(
                forApp: certificate,
                contentIdentifier: contentIdentifier,
                options: nil
            ) { spcData, error in
                if let spcData {
                    continuation.resume(returning: spcData)
                } else {
                    continuation.resume(
                        throwing: error ?? VesperPlayerDrmRuntimeError(
                            reason: "fairPlaySpcRequestFailed",
                            route: "direct",
                            keySystem: self.drmConfiguration.keySystem,
                            message: "FairPlay SPC request failed.",
                            retriable: true,
                            details: ["errorClass": "AVContentKeyRequest"]
                        )
                    )
                }
            }
        }
    }

    private func normalizeFairPlayError(
        _ error: Error,
        fallbackReason: String
    ) -> VesperPlayerDrmRuntimeError {
        if let drmError = error as? VesperPlayerDrmRuntimeError {
            return drmError
        }
        return VesperPlayerDrmRuntimeError(
            reason: fallbackReason,
            route: "direct",
            keySystem: drmConfiguration.keySystem,
            message: error.localizedDescription,
            retriable: true,
            details: fairPlayErrorDetails(from: error)
        )
    }

    private func report(_ error: Error) {
        onError(error)
    }

    private func takePendingTasks() -> [Task<Void, Never>] {
        stateLock.lock()
        defer { stateLock.unlock() }
        let tasks = Array(pendingTasks.values)
        pendingTasks.removeAll()
        return tasks
    }

    private func removePendingTask(_ requestId: ObjectIdentifier) {
        stateLock.lock()
        pendingTasks.removeValue(forKey: requestId)
        stateLock.unlock()
    }
}

func fairPlayContentIdentifierData(from identifier: Any?, fallback: String) -> Data {
    if let data = identifier as? Data {
        return data
    }
    if let url = identifier as? URL {
        return fairPlayContentIdentifierData(from: url.absoluteString, fallback: fallback)
    }
    if let string = identifier as? String {
        if let url = URL(string: string),
           url.scheme?.lowercased() == "skd" {
            return Data(string.utf8)
        }
        return Data(string.utf8)
    }
    return Data(fallback.utf8)
}

private func validateFairPlayHTTPResponse(
    _ response: URLResponse,
    reason: String,
    route: String,
    keySystem: String,
    details: [String: String] = [:]
) throws {
    guard let http = response as? HTTPURLResponse else {
        return
    }
    guard (200..<300).contains(http.statusCode) else {
        throw VesperPlayerDrmRuntimeError(
            reason: reason,
            route: route,
            keySystem: keySystem,
            message: "FairPlay HTTP request failed with status \(http.statusCode).",
            retriable: true,
            details: details.merging([
                "errorClass": "HTTPURLResponse",
                "httpStatusCode": "\(http.statusCode)",
            ]) { current, _ in current }
        )
    }
}

private func fairPlayErrorDetails(
    from error: Error,
    merging additionalDetails: [String: String] = [:]
) -> [String: String] {
    let nsError = error as NSError
    let reportedErrorClass: String
    if nsError.domain == NSURLErrorDomain {
        reportedErrorClass = "URLError"
    } else if let underlyingError = nsError.userInfo[NSUnderlyingErrorKey] as? Error {
        reportedErrorClass = String(describing: type(of: underlyingError))
    } else {
        reportedErrorClass = String(describing: type(of: error))
    }
    var values = [
        "errorClass": reportedErrorClass,
        "errorMessage": error.localizedDescription,
    ]
    values.merge(additionalDetails) { current, _ in current }
    return values
}

extension String {
    var vesperTrimmedNonEmpty: String? {
        let value = trimmingCharacters(in: .whitespacesAndNewlines)
        return value.isEmpty ? nil : value
    }
}
