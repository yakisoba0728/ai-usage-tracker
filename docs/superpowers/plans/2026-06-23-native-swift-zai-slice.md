# Native Swift z.ai Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a new native macOS Swift app alongside the existing Tauri app, proving one real z.ai usage refresh path from credential to `MenuBarExtra` and dashboard.

**Architecture:** Add a new `apps/macos-native/` Xcode macOS app with an internal `Packages/UsageCore` SwiftPM package. `UsageCore` owns tested models, redaction, z.ai parsing, native storage, provider client contracts, and refresh coordination; the app target owns SwiftUI scenes and macOS lifecycle. Existing Tauri paths (`src/`, `src-tauri/`, `scripts/tauri.mjs`, `package.json`) remain unchanged in this slice.

**Tech Stack:** Swift 6.4, SwiftPM, XCTest, SwiftUI, Xcode macOS app target, URLSession, Foundation actors, optional narrow AppKit interop.

---

## Source Spec

Implement against [docs/superpowers/specs/2026-06-23-native-swift-zai-slice-design.md](/Users/yakisoba/Documents/GitHub/ai-usage-tracker/docs/superpowers/specs/2026-06-23-native-swift-zai-slice-design.md).

## File Structure

Create these files and keep them focused:

```text
apps/macos-native/
  AIUsageTracker.xcodeproj/
    project.pbxproj
  AIUsageTracker/
    App/AIUsageTrackerApp.swift
    App/AppDelegate.swift
    Scenes/MenuBarStatusView.swift
    Scenes/DashboardWindowView.swift
    Features/Dashboard/DashboardView.swift
    Features/Dashboard/AccountListView.swift
    Features/Dashboard/AccountDetailView.swift
    Features/AddAccount/AddZaiAccountSheet.swift
    Features/Settings/NativeSettingsView.swift
    Stores/AppStore.swift
    Resources/Info.plist
    AIUsageTracker.entitlements
  AIUsageTrackerTests/
    AppStoreTests.swift
  Packages/UsageCore/
    Package.swift
    Sources/UsageCore/Models.swift
    Sources/UsageCore/Redactor.swift
    Sources/UsageCore/ZaiParser.swift
    Sources/UsageCore/Storage.swift
    Sources/UsageCore/ProviderClient.swift
    Sources/UsageCore/ZaiProviderClient.swift
    Sources/UsageCore/RefreshCoordinator.swift
    Sources/UsageCore/Projection.swift
    Tests/UsageCoreTests/ModelContractTests.swift
    Tests/UsageCoreTests/RedactorTests.swift
    Tests/UsageCoreTests/ZaiParserTests.swift
    Tests/UsageCoreTests/StorageTests.swift
    Tests/UsageCoreTests/RefreshCoordinatorTests.swift
    Tests/UsageCoreTests/ProjectionTests.swift
    Tests/UsageCoreTests/Fixtures/zai_quota_fixture.json
scripts/macos-native-run.sh
```

Do not add a root `Package.swift`. Do not edit existing Tauri app files in this plan.

## Task 1: UsageCore Package Scaffold And Model Contract

**Files:**
- Create: `apps/macos-native/Packages/UsageCore/Package.swift`
- Create: `apps/macos-native/Packages/UsageCore/Sources/UsageCore/Models.swift`
- Create: `apps/macos-native/Packages/UsageCore/Tests/UsageCoreTests/ModelContractTests.swift`

- [ ] **Step 1: Create the SwiftPM package scaffold**

Create `apps/macos-native/Packages/UsageCore/Package.swift`:

```swift
// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "UsageCore",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .library(name: "UsageCore", targets: ["UsageCore"])
    ],
    targets: [
        .target(name: "UsageCore"),
        .testTarget(name: "UsageCoreTests", dependencies: ["UsageCore"])
    ]
)
```

Create `apps/macos-native/Packages/UsageCore/Sources/UsageCore/Models.swift` with only the module import so the package exists:

```swift
import Foundation
```

- [ ] **Step 2: Write the failing model contract tests**

Create `apps/macos-native/Packages/UsageCore/Tests/UsageCoreTests/ModelContractTests.swift`:

```swift
import XCTest
@testable import UsageCore

final class ModelContractTests: XCTestCase {
    func testProviderOrderMatchesTauriContract() {
        XCTAssertEqual(Provider.allCases.map(\.rawValue), [
            "claude", "codex", "gemini", "copilot", "cursor", "zai"
        ])
    }

    func testServiceUsageDecodesSnakeCaseContract() throws {
        let json = """
        {
          "id": "auto:zai",
          "source": "auto",
          "provider": "zai",
          "connected": true,
          "plan": "Max",
          "account": "z.ai workspace",
          "error": null,
          "windows": [
            {
              "label": "5-hour",
              "used_percent": 9,
              "resets_at": 1781897705,
              "used": null,
              "limit": null
            }
          ],
          "detail_windows": []
        }
        """.data(using: .utf8)!

        let usage = try JSONDecoder.usageCore.decode(ServiceUsage.self, from: json)

        XCTAssertEqual(usage.id, "auto:zai")
        XCTAssertEqual(usage.source, .auto)
        XCTAssertEqual(usage.provider, .zai)
        XCTAssertTrue(usage.connected)
        XCTAssertEqual(usage.plan, "Max")
        XCTAssertEqual(usage.account, "z.ai workspace")
        XCTAssertNil(usage.error)
        XCTAssertEqual(usage.windows.count, 1)
        XCTAssertEqual(usage.windows[0].label, "5-hour")
        XCTAssertEqual(usage.windows[0].usedPercent, 9)
        XCTAssertEqual(usage.windows[0].resetsAt, 1_781_897_705)
        XCTAssertTrue(usage.detailWindows.isEmpty)
    }

    func testServiceUsageEncodesSnakeCaseContract() throws {
        let usage = ServiceUsage(
            id: "stored:abc",
            source: .stored,
            provider: .zai,
            connected: false,
            plan: nil,
            account: "Work",
            error: ServiceError(code: "network", detail: "timeout"),
            windows: [],
            detailWindows: [],
            rawResponse: nil
        )

        let data = try JSONEncoder.usageCore.encode(usage)
        let object = try JSONSerialization.jsonObject(with: data) as! [String: Any]

        XCTAssertEqual(object["id"] as? String, "stored:abc")
        XCTAssertEqual(object["source"] as? String, "stored")
        XCTAssertEqual(object["provider"] as? String, "zai")
        XCTAssertEqual(object["connected"] as? Bool, false)
        XCTAssertTrue(object.keys.contains("detail_windows"))
        XCTAssertFalse(object.keys.contains("raw_response"))
    }

    func testUsageSnapshotRoundTrip() throws {
        let snapshot = UsageSnapshot(
            fetchedAt: 1_800_000_000,
            services: [
                ServiceUsage(
                    id: "auto:zai",
                    source: .auto,
                    provider: .zai,
                    connected: true,
                    plan: "Max",
                    account: nil,
                    error: nil,
                    windows: [
                        LimitWindow(
                            label: "Weekly",
                            usedPercent: 60,
                            resetsAt: 1_782_210_628,
                            used: nil,
                            limit: nil
                        )
                    ],
                    detailWindows: [],
                    rawResponse: nil
                )
            ]
        )

        let data = try JSONEncoder.usageCore.encode(snapshot)
        let decoded = try JSONDecoder.usageCore.decode(UsageSnapshot.self, from: data)

        XCTAssertEqual(decoded.fetchedAt, snapshot.fetchedAt)
        XCTAssertEqual(decoded.services.first?.provider, .zai)
        XCTAssertEqual(decoded.services.first?.windows.first?.label, "Weekly")
    }
}
```

- [ ] **Step 3: Run the test and verify RED**

Run:

```bash
swift test --package-path apps/macos-native/Packages/UsageCore --filter ModelContractTests
```

Expected: FAIL to compile with errors such as `cannot find 'Provider' in scope`, `cannot find 'ServiceUsage' in scope`, and `type 'JSONDecoder' has no member 'usageCore'`.

- [ ] **Step 4: Implement minimal model types**

Replace `apps/macos-native/Packages/UsageCore/Sources/UsageCore/Models.swift`:

```swift
import Foundation

public enum Provider: String, Codable, CaseIterable, Sendable {
    case claude
    case codex
    case gemini
    case copilot
    case cursor
    case zai
}

public enum ServiceSource: String, Codable, Sendable {
    case auto
    case stored
}

public struct LimitWindow: Codable, Equatable, Sendable {
    public var label: String
    public var usedPercent: Double?
    public var resetsAt: Int64?
    public var used: Double?
    public var limit: Double?

    public init(
        label: String,
        usedPercent: Double?,
        resetsAt: Int64?,
        used: Double?,
        limit: Double?
    ) {
        self.label = label
        self.usedPercent = usedPercent
        self.resetsAt = resetsAt
        self.used = used
        self.limit = limit
    }

    enum CodingKeys: String, CodingKey {
        case label
        case usedPercent = "used_percent"
        case resetsAt = "resets_at"
        case used
        case limit
    }
}

public struct ServiceError: Codable, Equatable, Sendable {
    public var code: String
    public var detail: String?

    public init(code: String, detail: String?) {
        self.code = code
        self.detail = detail
    }
}

public struct ServiceUsage: Codable, Equatable, Sendable {
    public var id: String
    public var source: ServiceSource
    public var provider: Provider
    public var connected: Bool
    public var plan: String?
    public var account: String?
    public var error: ServiceError?
    public var windows: [LimitWindow]
    public var detailWindows: [LimitWindow]
    public var rawResponse: String?

    public init(
        id: String,
        source: ServiceSource,
        provider: Provider,
        connected: Bool,
        plan: String?,
        account: String?,
        error: ServiceError?,
        windows: [LimitWindow],
        detailWindows: [LimitWindow],
        rawResponse: String?
    ) {
        self.id = id
        self.source = source
        self.provider = provider
        self.connected = connected
        self.plan = plan
        self.account = account
        self.error = error
        self.windows = windows
        self.detailWindows = detailWindows
        self.rawResponse = rawResponse
    }

    enum CodingKeys: String, CodingKey {
        case id
        case source
        case provider
        case connected
        case plan
        case account
        case error
        case windows
        case detailWindows = "detail_windows"
        case rawResponse = "raw_response"
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(source, forKey: .source)
        try container.encode(provider, forKey: .provider)
        try container.encode(connected, forKey: .connected)
        try container.encodeIfPresent(plan, forKey: .plan)
        try container.encodeIfPresent(account, forKey: .account)
        try container.encodeIfPresent(error, forKey: .error)
        try container.encode(windows, forKey: .windows)
        try container.encode(detailWindows, forKey: .detailWindows)
        try container.encodeIfPresent(rawResponse, forKey: .rawResponse)
    }
}

public struct UsageSnapshot: Codable, Equatable, Sendable {
    public var fetchedAt: Int64
    public var services: [ServiceUsage]

    public init(fetchedAt: Int64, services: [ServiceUsage]) {
        self.fetchedAt = fetchedAt
        self.services = services
    }

    enum CodingKeys: String, CodingKey {
        case fetchedAt = "fetched_at"
        case services
    }
}

public struct AccountInfo: Codable, Equatable, Sendable {
    public var id: String
    public var provider: Provider
    public var label: String

    public init(id: String, provider: Provider, label: String) {
        self.id = id
        self.provider = provider
        self.label = label
    }
}

public extension JSONDecoder {
    static var usageCore: JSONDecoder {
        JSONDecoder()
    }
}

public extension JSONEncoder {
    static var usageCore: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return encoder
    }
}
```

- [ ] **Step 5: Run the test and verify GREEN**

Run:

```bash
swift test --package-path apps/macos-native/Packages/UsageCore --filter ModelContractTests
```

Expected: PASS.

- [ ] **Step 6: Commit Task 1**

```bash
git add apps/macos-native/Packages/UsageCore
git commit -m "feat: add native usage core model contracts"
```

## Task 2: Redaction And z.ai Parser

**Files:**
- Modify: `apps/macos-native/Packages/UsageCore/Package.swift`
- Create: `apps/macos-native/Packages/UsageCore/Sources/UsageCore/Redactor.swift`
- Create: `apps/macos-native/Packages/UsageCore/Sources/UsageCore/ZaiParser.swift`
- Create: `apps/macos-native/Packages/UsageCore/Tests/UsageCoreTests/RedactorTests.swift`
- Create: `apps/macos-native/Packages/UsageCore/Tests/UsageCoreTests/ZaiParserTests.swift`
- Create: `apps/macos-native/Packages/UsageCore/Tests/UsageCoreTests/Fixtures/zai_quota_fixture.json`

- [ ] **Step 1: Copy the z.ai fixture**

Create `apps/macos-native/Packages/UsageCore/Tests/UsageCoreTests/Fixtures/zai_quota_fixture.json` with the exact contents of `src-tauri/tests/zai_quota_fixture.json`.

Modify `apps/macos-native/Packages/UsageCore/Package.swift` so the test target processes fixtures:

```swift
.testTarget(
    name: "UsageCoreTests",
    dependencies: ["UsageCore"],
    resources: [.process("Fixtures")]
)
```

- [ ] **Step 2: Write failing redaction tests**

Create `apps/macos-native/Packages/UsageCore/Tests/UsageCoreTests/RedactorTests.swift`:

```swift
import XCTest
@testable import UsageCore

final class RedactorTests: XCTestCase {
    func testScrubsBearerTokensAndApiKeysFromText() {
        let input = "Authorization: Bearer sk-abc123 and key zai-secret-987"
        let output = Redactor.scrub(input)

        XCTAssertFalse(output.contains("sk-abc123"))
        XCTAssertFalse(output.contains("zai-secret-987"))
        XCTAssertTrue(output.contains("[redacted]"))
    }

    func testRedactsSensitiveJSONKeys() throws {
        let input = """
        {
          "access_token": "token-value",
          "authorization": "Bearer token-value",
          "nested": { "api_key": "zai-secret-987" },
          "safe": "visible"
        }
        """

        let output = try Redactor.redactedJSONString(from: input.data(using: .utf8)!)

        XCTAssertFalse(output.contains("token-value"))
        XCTAssertFalse(output.contains("zai-secret-987"))
        XCTAssertTrue(output.contains("visible"))
        XCTAssertTrue(output.contains("[redacted]"))
    }
}
```

- [ ] **Step 3: Write failing z.ai parser tests**

Create `apps/macos-native/Packages/UsageCore/Tests/UsageCoreTests/ZaiParserTests.swift`:

```swift
import XCTest
@testable import UsageCore

final class ZaiParserTests: XCTestCase {
    func testLiveFixtureCardShowsFiveHourThenWeekly() throws {
        let data = try fixtureData("zai_quota_fixture")
        let parsed = try ZaiParser.parse(data)

        XCTAssertEqual(parsed.plan, "Max")
        XCTAssertEqual(parsed.windows.count, 2)
        XCTAssertEqual(parsed.windows[0].label, "5-hour")
        XCTAssertEqual(parsed.windows[0].usedPercent, 9)
        XCTAssertEqual(parsed.windows[0].resetsAt, 1_781_897_705)
        XCTAssertEqual(parsed.windows[1].label, "Weekly")
        XCTAssertEqual(parsed.windows[1].usedPercent, 60)
        XCTAssertNil(parsed.windows[1].used)
        XCTAssertNil(parsed.windows[1].limit)
        XCTAssertEqual(parsed.windows[1].resetsAt, 1_782_210_628)
    }

    func testLiveFixtureDetailHasMonthlyAndModelRows() throws {
        let data = try fixtureData("zai_quota_fixture")
        let parsed = try ZaiParser.parse(data)

        XCTAssertFalse(parsed.detailWindows.contains { $0.label == "5-hour" })
        let monthly = try XCTUnwrap(parsed.detailWindows.first { $0.label == "Monthly" })
        XCTAssertEqual(monthly.usedPercent, 1)
        XCTAssertEqual(monthly.used, 7)
        XCTAssertEqual(monthly.limit, 4000)
        XCTAssertEqual(monthly.resetsAt, 1_783_852_228)
        XCTAssertTrue(parsed.detailWindows.contains { $0.label == "search-prime" && $0.used == 1 })
        XCTAssertTrue(parsed.detailWindows.contains { $0.label == "web-reader" && $0.used == 0 })
        XCTAssertTrue(parsed.detailWindows.contains { $0.label == "zread" && $0.used == 6 })
    }

    func testExhaustedUsageWindowCarriesNextFlushTime() throws {
        let data = """
        { "code": 1308, "data": { "next_flush_time": 1712956800000 } }
        """.data(using: .utf8)!

        let parsed = try ZaiParser.parse(data)

        XCTAssertEqual(parsed.windows.count, 1)
        XCTAssertEqual(parsed.windows[0].label, "5-hour")
        XCTAssertEqual(parsed.windows[0].usedPercent, 100)
        XCTAssertEqual(parsed.windows[0].resetsAt, 1_712_956_800)
    }

    func testExhaustedPeriodWindowParsesDatetimeFlushTime() throws {
        let data = """
        { "code": 1310, "data": { "next_flush_time": "2024-04-12 00:00:00" } }
        """.data(using: .utf8)!

        let parsed = try ZaiParser.parse(data)

        XCTAssertEqual(parsed.windows.count, 1)
        XCTAssertEqual(parsed.windows[0].label, "Period")
        XCTAssertEqual(parsed.windows[0].usedPercent, 100)
        XCTAssertEqual(parsed.windows[0].resetsAt, 1_712_880_000)
    }

    func testUnsuccessfulBusinessResponseThrowsServerError() {
        let data = """
        { "code": 4001, "success": false, "msg": "bad key" }
        """.data(using: .utf8)!

        XCTAssertThrowsError(try ZaiParser.parse(data)) { error in
            XCTAssertEqual(error as? ProviderError, .serverError("bad key"))
        }
    }

    private func fixtureData(_ name: String) throws -> Data {
        let url = try XCTUnwrap(Bundle.module.url(forResource: name, withExtension: "json"))
        return try Data(contentsOf: url)
    }
}
```

- [ ] **Step 4: Run tests and verify RED**

Run:

```bash
swift test --package-path apps/macos-native/Packages/UsageCore --filter RedactorTests
swift test --package-path apps/macos-native/Packages/UsageCore --filter ZaiParserTests
```

Expected: FAIL to compile with `cannot find 'Redactor' in scope`, `cannot find 'ZaiParser' in scope`, and `cannot find 'ProviderError' in scope`.

- [ ] **Step 5: Implement `ProviderError` in `Models.swift`**

Append to `Models.swift`:

```swift
public enum ProviderError: Error, Equatable, Sendable {
    case notLoggedIn(String)
    case expired(String)
    case network(String)
    case serverError(String)
    case parse(String)

    public var code: String {
        switch self {
        case .notLoggedIn: "not_logged_in"
        case .expired: "token_expired"
        case .network: "network"
        case .serverError: "server_error"
        case .parse: "parse_error"
        }
    }

    public var detail: String {
        switch self {
        case .notLoggedIn(let message),
             .expired(let message),
             .network(let message),
             .serverError(let message),
             .parse(let message):
            message
        }
    }
}
```

- [ ] **Step 6: Implement `Redactor`**

Create `apps/macos-native/Packages/UsageCore/Sources/UsageCore/Redactor.swift`:

```swift
import Foundation

public enum Redactor {
    private static let redacted = "[redacted]"
    private static let sensitiveKeys = [
        "token", "sessionkey", "apikey", "key", "authorization", "cookie",
        "email", "emailaddress", "userid", "accountid"
    ]

    public static func scrub(_ text: String) -> String {
        var output = text
        let patterns = [
            #"Bearer\s+[A-Za-z0-9._\-]+"#,
            #"sk-[A-Za-z0-9._\-]+"#,
            #"zai-[A-Za-z0-9._\-]+"#,
            #"Authorization:\s*[^\n\r]+"#
        ]

        for pattern in patterns {
            output = output.replacingOccurrences(
                of: pattern,
                with: redacted,
                options: [.regularExpression, .caseInsensitive]
            )
        }
        return output
    }

    public static func redactedJSONString(from data: Data) throws -> String {
        let object = try JSONSerialization.jsonObject(with: data)
        let redactedObject = redactJSONValue(object)
        let output = try JSONSerialization.data(withJSONObject: redactedObject, options: [.prettyPrinted, .sortedKeys])
        return String(decoding: output, as: UTF8.self)
    }

    private static func redactJSONValue(_ value: Any) -> Any {
        if let dict = value as? [String: Any] {
            var next: [String: Any] = [:]
            for (key, value) in dict {
                next[key] = isSensitiveKey(key) ? redacted : redactJSONValue(value)
            }
            return next
        }
        if let array = value as? [Any] {
            return array.map(redactJSONValue)
        }
        if let string = value as? String {
            return scrub(string)
        }
        return value
    }

    private static func isSensitiveKey(_ key: String) -> Bool {
        let normalized = key
            .filter { $0 != "-" && $0 != "_" }
            .lowercased()
        return sensitiveKeys.contains { normalized.contains($0) }
    }
}
```

- [ ] **Step 7: Implement `ZaiParser`**

Create `apps/macos-native/Packages/UsageCore/Sources/UsageCore/ZaiParser.swift`:

```swift
import Foundation

public enum ZaiParser {
    private static let usageExhaustedCode = 1308
    private static let periodExhaustedCode = 1310

    public struct ParsedUsage: Equatable, Sendable {
        public var plan: String?
        public var windows: [LimitWindow]
        public var detailWindows: [LimitWindow]
        public var rawResponse: String?
    }

    public static func parse(_ data: Data) throws -> ParsedUsage {
        let rawObject = try JSONSerialization.jsonObject(with: data)
        let rawResponse = try? Redactor.redactedJSONString(from: data)
        let response = try JSONDecoder.usageCore.decode(Response.self, from: data)

        if let code = response.code, code == usageExhaustedCode || code == periodExhaustedCode {
            return ParsedUsage(
                plan: plan(from: response.data?.level),
                windows: [exhaustedWindow(code: code, data: rawObject)],
                detailWindows: [],
                rawResponse: rawResponse
            )
        }

        let ok = response.success == true || response.code == 200
        guard ok else {
            throw ProviderError.serverError(response.msg ?? "z.ai quota request failed")
        }

        let normalized = normalize(response.data)
        return ParsedUsage(
            plan: plan(from: response.data?.level),
            windows: normalized.windows,
            detailWindows: normalized.detailWindows,
            rawResponse: rawResponse
        )
    }

    private static func normalize(_ data: DataPayload?) -> (windows: [LimitWindow], detailWindows: [LimitWindow]) {
        guard let data else { return ([], []) }
        var fiveHour: LimitWindow?
        var weekly: LimitWindow?
        var detail: [LimitWindow] = []

        for entry in data.limits {
            let slot = classify(entry)
            if let window = window(from: entry, slot: slot) {
                switch slot {
                case .fiveHour where fiveHour == nil:
                    fiveHour = window
                case .weekly where weekly == nil:
                    weekly = window
                default:
                    detail.append(window)
                }
            }

            if slot == .detail {
                for usage in entry.usageDetails {
                    if let modelCode = usage.modelCode, let used = usage.usage {
                        detail.append(
                            LimitWindow(label: modelCode, usedPercent: nil, resetsAt: nil, used: used, limit: nil)
                        )
                    }
                }
            }
        }

        return ([fiveHour, weekly].compactMap { $0 }, detail)
    }

    private static func classify(_ entry: LimitEntry) -> Slot {
        let raw = entry.rawType ?? ""
        let type = (entry.type ?? "").lowercased()
        let unit = entry.unit ?? 0

        if raw == "TOKENS_LIMIT" || raw.isEmpty {
            if unit == 3 || type.contains("5h") || type.contains("5 hour") || type.contains("5-hour") {
                return .fiveHour
            }
            if unit == 6 || type.contains("week") {
                return .weekly
            }
            if raw == "TOKENS_LIMIT" {
                return .detail
            }
        }
        return .detail
    }

    private static func window(from entry: LimitEntry, slot: Slot) -> LimitWindow? {
        guard let label = label(for: entry, slot: slot) else { return nil }
        let limit = entry.usage ?? entry.total
        let used = entry.currentValue ?? {
            guard let limit, let remaining = entry.remaining else { return nil }
            return max(limit - remaining, 0)
        }()
        let usedPercent = entry.percentage ?? {
            guard let used, let limit, limit > 0 else { return nil }
            return used / limit * 100
        }()
        let resetsAt = entry.nextResetTime.flatMap(normalizeEpoch)

        if usedPercent == nil, resetsAt == nil, used == nil, limit == nil {
            return nil
        }

        return LimitWindow(label: label, usedPercent: usedPercent, resetsAt: resetsAt, used: used, limit: limit)
    }

    private static func label(for entry: LimitEntry, slot: Slot) -> String? {
        switch slot {
        case .fiveHour:
            return "5-hour"
        case .weekly:
            return "Weekly"
        case .detail:
            switch entry.unit ?? 0 {
            case 3: return "5-hour"
            case 5: return "Monthly"
            case 6: return "Weekly"
            default: return entry.type?.isEmpty == false ? entry.type : entry.rawType
            }
        }
    }

    private static func exhaustedWindow(code: Int, data: Any) -> LimitWindow {
        let label = code == periodExhaustedCode ? "Period" : "5-hour"
        let object = data as? [String: Any]
        let dataObject = object?["data"] as? [String: Any]
        let resetsAt = dataObject?["next_flush_time"].flatMap(parseNextFlush)
        return LimitWindow(label: label, usedPercent: 100, resetsAt: resetsAt, used: nil, limit: nil)
    }

    private static func parseNextFlush(_ value: Any) -> Int64? {
        if let int = value as? Int64 { return normalizeEpoch(int) }
        if let int = value as? Int { return normalizeEpoch(Int64(int)) }
        if let double = value as? Double { return normalizeEpoch(Int64(double)) }
        guard let string = value as? String else { return nil }
        if let int = Int64(string) { return normalizeEpoch(int) }
        let isoFormatter = ISO8601DateFormatter()
        if let date = isoFormatter.date(from: string) {
            return Int64(date.timeIntervalSince1970)
        }
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        formatter.dateFormat = "yyyy-MM-dd HH:mm:ss"
        return formatter.date(from: string).map { Int64($0.timeIntervalSince1970) }
    }

    private static func normalizeEpoch(_ value: Int64) -> Int64 {
        abs(value) > 1_000_000_000_000 ? value / 1000 : value
    }

    private static func plan(from level: String?) -> String? {
        guard let trimmed = level?.trimmingCharacters(in: .whitespacesAndNewlines),
              !trimmed.isEmpty,
              trimmed.lowercased() != "unknown"
        else {
            return nil
        }
        return trimmed.prefix(1).uppercased() + trimmed.dropFirst()
    }

    private enum Slot {
        case fiveHour
        case weekly
        case detail
    }

    private struct Response: Decodable {
        var code: Int?
        var success: Bool?
        var msg: String?
        var data: DataPayload?
    }

    private struct DataPayload: Decodable {
        var level: String?
        var limits: [LimitEntry]

        init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            level = try container.decodeIfPresent(String.self, forKey: .level)
            limits = try container.decodeIfPresent([LimitEntry].self, forKey: .limits) ?? []
        }

        private enum CodingKeys: String, CodingKey {
            case level
            case limits
        }
    }

    private struct LimitEntry: Decodable {
        var type: String?
        var rawType: String?
        var unit: Int?
        var usage: Double?
        var total: Double?
        var currentValue: Double?
        var remaining: Double?
        var percentage: Double?
        var nextResetTime: Int64?
        var usageDetails: [UsageDetail]

        private enum CodingKeys: String, CodingKey {
            case type
            case rawType
            case unit
            case usage
            case total
            case currentValue
            case remaining
            case percentage
            case nextResetTime
            case usageDetails
        }

        init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            type = try container.decodeIfPresent(String.self, forKey: .type)
            rawType = try container.decodeIfPresent(String.self, forKey: .rawType)
            unit = try container.decodeIfPresent(Int.self, forKey: .unit)
            usage = try container.decodeIfPresent(Double.self, forKey: .usage)
            total = try container.decodeIfPresent(Double.self, forKey: .total)
            currentValue = try container.decodeIfPresent(Double.self, forKey: .currentValue)
            remaining = try container.decodeIfPresent(Double.self, forKey: .remaining)
            percentage = try container.decodeIfPresent(Double.self, forKey: .percentage)
            nextResetTime = try container.decodeIfPresent(Int64.self, forKey: .nextResetTime)
            usageDetails = try container.decodeIfPresent([UsageDetail].self, forKey: .usageDetails) ?? []
        }
    }

    private struct UsageDetail: Decodable {
        var modelCode: String?
        var usage: Double?
    }
}
```

- [ ] **Step 8: Run tests and verify GREEN**

Run:

```bash
swift test --package-path apps/macos-native/Packages/UsageCore --filter RedactorTests
swift test --package-path apps/macos-native/Packages/UsageCore --filter ZaiParserTests
swift test --package-path apps/macos-native/Packages/UsageCore
```

Expected: PASS.

- [ ] **Step 9: Commit Task 2**

```bash
git add apps/macos-native/Packages/UsageCore
git commit -m "feat: port z.ai usage parsing to Swift"
```

## Task 3: Native Config And Account Stores

**Files:**
- Create: `apps/macos-native/Packages/UsageCore/Sources/UsageCore/Storage.swift`
- Create: `apps/macos-native/Packages/UsageCore/Tests/UsageCoreTests/StorageTests.swift`

- [ ] **Step 1: Write failing storage tests**

Create `apps/macos-native/Packages/UsageCore/Tests/UsageCoreTests/StorageTests.swift`:

```swift
import XCTest
@testable import UsageCore

final class StorageTests: XCTestCase {
    func testConfigStoreRoundTripsNativeConfigInIsolatedDirectory() async throws {
        let root = try temporaryDirectory()
        let store = ConfigStore(rootDirectory: root)
        var config = AppConfig.default
        config.pollSeconds = 120
        config.showOffline = true

        try await store.save(config)
        let loaded = try await store.load()

        XCTAssertEqual(loaded.pollSeconds, 120)
        XCTAssertTrue(loaded.showOffline)
        XCTAssertTrue(FileManager.default.fileExists(atPath: root.appending(path: "config.json").path))
    }

    func testAccountStoreRoundTripsZaiAccountWithoutTauriPath() async throws {
        let root = try temporaryDirectory()
        let store = AccountStore(rootDirectory: root)
        let account = StoredAccount(
            id: "zai-1",
            provider: .zai,
            label: "Work z.ai",
            credential: "zai-secret-987"
        )

        try await store.save([account])
        let loaded = try await store.load()

        XCTAssertEqual(loaded, [account])
        XCTAssertTrue(FileManager.default.fileExists(atPath: root.appending(path: "accounts.json").path))
    }

    func testAccountInfoNeverContainsCredential() {
        let account = StoredAccount(
            id: "zai-1",
            provider: .zai,
            label: "Work z.ai",
            credential: "zai-secret-987"
        )

        let info = account.accountInfo

        XCTAssertEqual(info.id, "zai-1")
        XCTAssertEqual(info.provider, .zai)
        XCTAssertEqual(info.label, "Work z.ai")
        XCTAssertFalse(String(describing: info).contains("zai-secret-987"))
    }

    func testCorruptAccountFileReportsParseError() async throws {
        let root = try temporaryDirectory()
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try "{not json".write(to: root.appending(path: "accounts.json"), atomically: true, encoding: .utf8)
        let store = AccountStore(rootDirectory: root)

        do {
            _ = try await store.load()
            XCTFail("Expected corrupt account file to throw")
        } catch let error as ProviderError {
            XCTAssertEqual(error.code, "parse_error")
        }
    }

    private func temporaryDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appending(path: "UsageCoreTests-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }
}
```

- [ ] **Step 2: Run test and verify RED**

Run:

```bash
swift test --package-path apps/macos-native/Packages/UsageCore --filter StorageTests
```

Expected: FAIL to compile with missing `ConfigStore`, `AppConfig`, `AccountStore`, and `StoredAccount`.

- [ ] **Step 3: Implement native stores**

Create `apps/macos-native/Packages/UsageCore/Sources/UsageCore/Storage.swift`:

```swift
import Foundation

public struct ProviderConfig: Codable, Equatable, Sendable {
    public var enabled: Bool
    public var notifyThresholds: [Int]
    public var sortIndex: Int

    public init(enabled: Bool, notifyThresholds: [Int], sortIndex: Int) {
        self.enabled = enabled
        self.notifyThresholds = notifyThresholds
        self.sortIndex = sortIndex
    }

    enum CodingKeys: String, CodingKey {
        case enabled
        case notifyThresholds = "notify_thresholds"
        case sortIndex = "sort_index"
    }
}

public struct AccountConfig: Codable, Equatable, Sendable {
    public var customName: String?
    public var primaryWindow: String?

    public init(customName: String? = nil, primaryWindow: String? = nil) {
        self.customName = customName
        self.primaryWindow = primaryWindow
    }

    enum CodingKeys: String, CodingKey {
        case customName = "custom_name"
        case primaryWindow = "primary_window"
    }
}

public struct AppConfig: Codable, Equatable, Sendable {
    public var schemaVersion: Int
    public var pollSeconds: Int
    public var providers: [ProviderConfig]
    public var accounts: [String: AccountConfig]
    public var showOffline: Bool

    public static var `default`: AppConfig {
        AppConfig(
            schemaVersion: 1,
            pollSeconds: 300,
            providers: Provider.allCases.enumerated().map { index, _ in
                ProviderConfig(enabled: true, notifyThresholds: [50, 75, 90, 95, 100], sortIndex: index)
            },
            accounts: [:],
            showOffline: false
        )
    }

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case pollSeconds = "poll_seconds"
        case providers
        case accounts
        case showOffline = "show_offline"
    }
}

public struct StoredAccount: Codable, Equatable, Sendable {
    public var id: String
    public var provider: Provider
    public var label: String
    public var credential: String

    public init(id: String, provider: Provider, label: String, credential: String) {
        self.id = id
        self.provider = provider
        self.label = label
        self.credential = credential
    }

    public var accountInfo: AccountInfo {
        AccountInfo(id: id, provider: provider, label: label)
    }
}

public actor ConfigStore {
    private let url: URL

    public init(rootDirectory: URL) {
        self.url = rootDirectory.appending(path: "config.json")
    }

    public func load() async throws -> AppConfig {
        guard FileManager.default.fileExists(atPath: url.path) else {
            return .default
        }
        do {
            let data = try Data(contentsOf: url)
            return try JSONDecoder.usageCore.decode(AppConfig.self, from: data)
        } catch {
            throw ProviderError.parse("native config could not be parsed: \(error)")
        }
    }

    public func save(_ config: AppConfig) async throws {
        try writeAtomic(config, to: url)
    }
}

public actor AccountStore {
    private let url: URL

    public init(rootDirectory: URL) {
        self.url = rootDirectory.appending(path: "accounts.json")
    }

    public func load() async throws -> [StoredAccount] {
        guard FileManager.default.fileExists(atPath: url.path) else {
            return []
        }
        do {
            let data = try Data(contentsOf: url)
            return try JSONDecoder.usageCore.decode([StoredAccount].self, from: data)
        } catch {
            throw ProviderError.parse("native accounts could not be parsed: \(error)")
        }
    }

    public func save(_ accounts: [StoredAccount]) async throws {
        try writeAtomic(accounts, to: url)
    }
}

private func writeAtomic<T: Encodable>(_ value: T, to url: URL) throws {
    let directory = url.deletingLastPathComponent()
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let data = try JSONEncoder.usageCore.encode(value)
    let tmp = directory.appending(path: ".\(url.lastPathComponent).\(UUID().uuidString).tmp")
    try data.write(to: tmp, options: [.atomic])
    if FileManager.default.fileExists(atPath: url.path) {
        try FileManager.default.removeItem(at: url)
    }
    try FileManager.default.moveItem(at: tmp, to: url)
}
```

- [ ] **Step 4: Run tests and verify GREEN**

Run:

```bash
swift test --package-path apps/macos-native/Packages/UsageCore --filter StorageTests
swift test --package-path apps/macos-native/Packages/UsageCore
```

Expected: PASS.

- [ ] **Step 5: Commit Task 3**

```bash
git add apps/macos-native/Packages/UsageCore
git commit -m "feat: add isolated native config and account stores"
```

## Task 4: z.ai Provider Client And Refresh Coordinator

**Files:**
- Create: `apps/macos-native/Packages/UsageCore/Sources/UsageCore/ProviderClient.swift`
- Create: `apps/macos-native/Packages/UsageCore/Sources/UsageCore/ZaiProviderClient.swift`
- Create: `apps/macos-native/Packages/UsageCore/Sources/UsageCore/RefreshCoordinator.swift`
- Create: `apps/macos-native/Packages/UsageCore/Tests/UsageCoreTests/RefreshCoordinatorTests.swift`

- [ ] **Step 1: Write failing refresh coordinator tests**

Create `apps/macos-native/Packages/UsageCore/Tests/UsageCoreTests/RefreshCoordinatorTests.swift`:

```swift
import XCTest
@testable import UsageCore

final class RefreshCoordinatorTests: XCTestCase {
    func testSuccessfulZaiRefreshBuildsConnectedSnapshot() async {
        let client = StubProviderClient(
            provider: .zai,
            result: .success(
                ServiceUsage(
                    id: "auto:zai",
                    source: .auto,
                    provider: .zai,
                    connected: true,
                    plan: "Max",
                    account: nil,
                    error: nil,
                    windows: [
                        LimitWindow(label: "5-hour", usedPercent: 9, resetsAt: 1_781_897_705, used: nil, limit: nil)
                    ],
                    detailWindows: [],
                    rawResponse: nil
                )
            )
        )
        let coordinator = RefreshCoordinator(providerClients: [client], clock: { 1_800_000_000 })

        let snapshot = await coordinator.refreshAll()

        XCTAssertEqual(snapshot.fetchedAt, 1_800_000_000)
        XCTAssertEqual(snapshot.services.count, 1)
        XCTAssertTrue(snapshot.services[0].connected)
        XCTAssertEqual(snapshot.services[0].provider, .zai)
    }

    func testProviderFailureBecomesDisconnectedService() async {
        let client = StubProviderClient(provider: .zai, result: .failure(.network("offline")))
        let coordinator = RefreshCoordinator(providerClients: [client], clock: { 1_800_000_000 })

        let snapshot = await coordinator.refreshAll()

        XCTAssertEqual(snapshot.services.count, 1)
        XCTAssertFalse(snapshot.services[0].connected)
        XCTAssertEqual(snapshot.services[0].error?.code, "network")
        XCTAssertEqual(snapshot.services[0].id, "auto:zai")
    }

    func testConcurrentRefreshesShareOneInFlightRefresh() async {
        let client = CountingProviderClient(provider: .zai)
        let coordinator = RefreshCoordinator(providerClients: [client], clock: { 1_800_000_000 })

        async let first = coordinator.refreshAll()
        async let second = coordinator.refreshAll()
        _ = await [first, second]

        let count = await client.callCount
        XCTAssertEqual(count, 1)
    }
}

private struct StubProviderClient: ProviderClient {
    let provider: Provider
    let result: Result<ServiceUsage, ProviderError>

    func fetch() async throws -> ServiceUsage {
        try result.get()
    }
}

private actor CountingProviderClient: ProviderClient {
    nonisolated let provider: Provider
    private(set) var callCount = 0

    init(provider: Provider) {
        self.provider = provider
    }

    func fetch() async throws -> ServiceUsage {
        callCount += 1
        try? await Task.sleep(for: .milliseconds(50))
        return ServiceUsage(
            id: "auto:zai",
            source: .auto,
            provider: .zai,
            connected: true,
            plan: nil,
            account: nil,
            error: nil,
            windows: [],
            detailWindows: [],
            rawResponse: nil
        )
    }
}
```

- [ ] **Step 2: Run test and verify RED**

Run:

```bash
swift test --package-path apps/macos-native/Packages/UsageCore --filter RefreshCoordinatorTests
```

Expected: FAIL to compile with missing `ProviderClient` and `RefreshCoordinator`.

- [ ] **Step 3: Implement provider protocol**

Create `apps/macos-native/Packages/UsageCore/Sources/UsageCore/ProviderClient.swift`:

```swift
import Foundation

public protocol ProviderClient: Sendable {
    var provider: Provider { get }
    func fetch() async throws -> ServiceUsage
}

public func autoServiceID(_ provider: Provider) -> String {
    "auto:\(provider.rawValue)"
}

public func storedServiceID(_ accountID: String) -> String {
    "stored:\(accountID)"
}
```

- [ ] **Step 4: Implement refresh coordinator**

Create `apps/macos-native/Packages/UsageCore/Sources/UsageCore/RefreshCoordinator.swift`:

```swift
import Foundation

public actor RefreshCoordinator {
    private let providerClients: [any ProviderClient]
    private let clock: @Sendable () -> Int64
    private var inFlight: Task<UsageSnapshot, Never>?

    public init(
        providerClients: [any ProviderClient],
        clock: @escaping @Sendable () -> Int64 = { Int64(Date().timeIntervalSince1970) }
    ) {
        self.providerClients = providerClients
        self.clock = clock
    }

    public func refreshAll() async -> UsageSnapshot {
        if let inFlight {
            return await inFlight.value
        }

        let task = Task { [providerClients, clock] in
            let services = await withTaskGroup(of: ServiceUsage.self) { group in
                for client in providerClients {
                    group.addTask {
                        do {
                            return try await client.fetch()
                        } catch let error as ProviderError {
                            return disconnectedUsage(provider: client.provider, error: error)
                        } catch {
                            return disconnectedUsage(provider: client.provider, error: .network(String(describing: error)))
                        }
                    }
                }

                var values: [ServiceUsage] = []
                for await usage in group {
                    values.append(usage)
                }
                return values.sorted { lhs, rhs in
                    Provider.allCases.firstIndex(of: lhs.provider)! < Provider.allCases.firstIndex(of: rhs.provider)!
                }
            }
            return UsageSnapshot(fetchedAt: clock(), services: services)
        }

        inFlight = task
        let snapshot = await task.value
        inFlight = nil
        return snapshot
    }
}

private func disconnectedUsage(provider: Provider, error: ProviderError) -> ServiceUsage {
    ServiceUsage(
        id: autoServiceID(provider),
        source: .auto,
        provider: provider,
        connected: false,
        plan: nil,
        account: nil,
        error: ServiceError(code: error.code, detail: error.detail),
        windows: [],
        detailWindows: [],
        rawResponse: nil
    )
}
```

- [ ] **Step 5: Write failing z.ai provider client test inside `RefreshCoordinatorTests`**

Append this test to `RefreshCoordinatorTests`:

```swift
func testZaiProviderClientSendsBearerTokenAndParsesResponse() async throws {
    let fixture = try fixtureData("zai_quota_fixture")
    let transport = StubHTTPTransport { request in
        XCTAssertEqual(request.url?.absoluteString, "https://api.z.ai/api/monitor/usage/quota/limit")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer zai-secret-987")
        return (fixture, HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!)
    }
    let client = ZaiProviderClient(apiKey: "zai-secret-987", label: "Work", transport: transport)

    let usage = try await client.fetch()

    XCTAssertEqual(usage.id, "auto:zai")
    XCTAssertEqual(usage.provider, .zai)
    XCTAssertEqual(usage.plan, "Max")
    XCTAssertEqual(usage.account, "Work")
    XCTAssertEqual(usage.windows.first?.label, "5-hour")
}

private func fixtureData(_ name: String) throws -> Data {
    let url = try XCTUnwrap(Bundle.module.url(forResource: name, withExtension: "json"))
    return try Data(contentsOf: url)
}

private struct StubHTTPTransport: HTTPTransport {
    let handler: @Sendable (URLRequest) async throws -> (Data, HTTPURLResponse)

    func data(for request: URLRequest) async throws -> (Data, HTTPURLResponse) {
        try await handler(request)
    }
}
```

- [ ] **Step 6: Run test and verify RED**

Run:

```bash
swift test --package-path apps/macos-native/Packages/UsageCore --filter RefreshCoordinatorTests/testZaiProviderClientSendsBearerTokenAndParsesResponse
```

Expected: FAIL to compile with missing `HTTPTransport` and `ZaiProviderClient`.

- [ ] **Step 7: Implement z.ai provider client**

Create `apps/macos-native/Packages/UsageCore/Sources/UsageCore/ZaiProviderClient.swift`:

```swift
import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

public protocol HTTPTransport: Sendable {
    func data(for request: URLRequest) async throws -> (Data, HTTPURLResponse)
}

public struct URLSessionTransport: HTTPTransport {
    public init() {}

    public func data(for request: URLRequest) async throws -> (Data, HTTPURLResponse) {
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse else {
            throw ProviderError.network("z.ai quota response was not HTTP")
        }
        return (data, http)
    }
}

public struct ZaiProviderClient: ProviderClient {
    public let provider: Provider = .zai
    private let apiKey: String
    private let label: String?
    private let transport: any HTTPTransport

    public init(apiKey: String, label: String?, transport: any HTTPTransport = URLSessionTransport()) {
        self.apiKey = apiKey
        self.label = label
        self.transport = transport
    }

    public func fetch() async throws -> ServiceUsage {
        guard !apiKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw ProviderError.notLoggedIn("z.ai API key not set")
        }

        var request = URLRequest(url: URL(string: "https://api.z.ai/api/monitor/usage/quota/limit")!)
        request.httpMethod = "GET"
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")

        let (data, response) = try await transport.data(for: request)
        guard (200..<300).contains(response.statusCode) else {
            throw ProviderError.serverError(Redactor.scrub("z.ai quota HTTP \(response.statusCode): \(String(decoding: data, as: UTF8.self))"))
        }

        let parsed = try ZaiParser.parse(data)
        return ServiceUsage(
            id: autoServiceID(.zai),
            source: .auto,
            provider: .zai,
            connected: true,
            plan: parsed.plan,
            account: label,
            error: nil,
            windows: parsed.windows,
            detailWindows: parsed.detailWindows,
            rawResponse: parsed.rawResponse
        )
    }
}
```

- [ ] **Step 8: Run tests and verify GREEN**

Run:

```bash
swift test --package-path apps/macos-native/Packages/UsageCore --filter RefreshCoordinatorTests
swift test --package-path apps/macos-native/Packages/UsageCore
```

Expected: PASS.

- [ ] **Step 9: Commit Task 4**

```bash
git add apps/macos-native/Packages/UsageCore
git commit -m "feat: add z.ai provider refresh coordinator"
```

## Task 5: Projection Logic For Menu Bar And Dashboard

**Files:**
- Create: `apps/macos-native/Packages/UsageCore/Sources/UsageCore/Projection.swift`
- Create: `apps/macos-native/Packages/UsageCore/Tests/UsageCoreTests/ProjectionTests.swift`

- [ ] **Step 1: Write failing projection tests**

Create `apps/macos-native/Packages/UsageCore/Tests/UsageCoreTests/ProjectionTests.swift`:

```swift
import XCTest
@testable import UsageCore

final class ProjectionTests: XCTestCase {
    func testRemainingHeadroomUsesOneHundredMinusUsedPercent() {
        let usage = ServiceUsage(
            id: "auto:zai",
            source: .auto,
            provider: .zai,
            connected: true,
            plan: nil,
            account: nil,
            error: nil,
            windows: [LimitWindow(label: "5-hour", usedPercent: 9, resetsAt: nil, used: nil, limit: nil)],
            detailWindows: [],
            rawResponse: nil
        )

        XCTAssertEqual(UsageProjection.remainingPercent(for: usage), 91)
        XCTAssertEqual(UsageProjection.menuBarTitle(for: usage), "z.ai · 91%")
    }

    func testDisconnectedServiceShowsProviderAndErrorCode() {
        let usage = ServiceUsage(
            id: "auto:zai",
            source: .auto,
            provider: .zai,
            connected: false,
            plan: nil,
            account: nil,
            error: ServiceError(code: "not_logged_in", detail: "missing key"),
            windows: [],
            detailWindows: [],
            rawResponse: nil
        )

        XCTAssertNil(UsageProjection.remainingPercent(for: usage))
        XCTAssertEqual(UsageProjection.subtitle(for: usage), "not_logged_in")
    }

    func testStoredUUIDIsNeverDisplayedAsSubtitle() {
        let usage = ServiceUsage(
            id: "stored:abc123",
            source: .stored,
            provider: .zai,
            connected: true,
            plan: "Max",
            account: nil,
            error: nil,
            windows: [],
            detailWindows: [],
            rawResponse: nil
        )

        XCTAssertEqual(UsageProjection.subtitle(for: usage), "Max")
        XCTAssertFalse(UsageProjection.subtitle(for: usage).contains("stored:"))
    }
}
```

- [ ] **Step 2: Run test and verify RED**

Run:

```bash
swift test --package-path apps/macos-native/Packages/UsageCore --filter ProjectionTests
```

Expected: FAIL to compile with missing `UsageProjection`.

- [ ] **Step 3: Implement projection logic**

Create `apps/macos-native/Packages/UsageCore/Sources/UsageCore/Projection.swift`:

```swift
import Foundation

public enum UsageProjection {
    public static func providerDisplayName(_ provider: Provider) -> String {
        switch provider {
        case .claude: "Claude"
        case .codex: "Codex"
        case .gemini: "Gemini"
        case .copilot: "Copilot"
        case .cursor: "Cursor"
        case .zai: "z.ai"
        }
    }

    public static func remainingPercent(for usage: ServiceUsage) -> Int? {
        guard let used = usage.windows.first?.usedPercent else { return nil }
        return Int((100 - used).rounded().clamped(to: 0...100))
    }

    public static func menuBarTitle(for usage: ServiceUsage) -> String {
        let name = providerDisplayName(usage.provider)
        if let remaining = remainingPercent(for: usage) {
            return "\(name) · \(remaining)%"
        }
        return name
    }

    public static func subtitle(for usage: ServiceUsage) -> String {
        if let account = usage.account, !account.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return account
        }
        if let plan = usage.plan, !plan.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return plan
        }
        if let code = usage.error?.code {
            return code
        }
        return providerDisplayName(usage.provider)
    }
}

private extension Comparable {
    func clamped(to range: ClosedRange<Self>) -> Self {
        min(max(self, range.lowerBound), range.upperBound)
    }
}
```

- [ ] **Step 4: Run tests and verify GREEN**

Run:

```bash
swift test --package-path apps/macos-native/Packages/UsageCore --filter ProjectionTests
swift test --package-path apps/macos-native/Packages/UsageCore
```

Expected: PASS.

- [ ] **Step 5: Commit Task 5**

```bash
git add apps/macos-native/Packages/UsageCore
git commit -m "feat: add native usage projection helpers"
```

## Task 6: Xcode macOS App Scaffold And Run Script

**Files:**
- Create: `apps/macos-native/AIUsageTracker.xcodeproj/project.pbxproj`
- Create: `apps/macos-native/AIUsageTracker/App/AIUsageTrackerApp.swift`
- Create: `apps/macos-native/AIUsageTracker/App/AppDelegate.swift`
- Create: `apps/macos-native/AIUsageTracker/Stores/AppStore.swift`
- Create: `apps/macos-native/AIUsageTracker/Resources/Info.plist`
- Create: `apps/macos-native/AIUsageTracker/AIUsageTracker.entitlements`
- Create: `apps/macos-native/AIUsageTrackerTests/AppStoreTests.swift`
- Create: `scripts/macos-native-run.sh`
- Modify: `.gitignore`

- [ ] **Step 1: Write failing app store test**

Create `apps/macos-native/AIUsageTrackerTests/AppStoreTests.swift`:

```swift
import XCTest
import UsageCore
@testable import AIUsageTracker

@MainActor
final class AppStoreTests: XCTestCase {
    func testRefreshLoadsSnapshotIntoStore() async {
        let usage = ServiceUsage(
            id: "auto:zai",
            source: .auto,
            provider: .zai,
            connected: true,
            plan: "Max",
            account: nil,
            error: nil,
            windows: [LimitWindow(label: "5-hour", usedPercent: 9, resetsAt: nil, used: nil, limit: nil)],
            detailWindows: [],
            rawResponse: nil
        )
        let coordinator = RefreshCoordinator(providerClients: [StaticProviderClient(usage: usage)], clock: { 1_800_000_000 })
        let store = AppStore(refreshCoordinator: coordinator)

        await store.refresh()

        XCTAssertEqual(store.snapshot?.fetchedAt, 1_800_000_000)
        XCTAssertEqual(store.snapshot?.services.first?.provider, .zai)
        XCTAssertFalse(store.isRefreshing)
    }
}

private struct StaticProviderClient: ProviderClient {
    let provider: Provider = .zai
    let usage: ServiceUsage

    func fetch() async throws -> ServiceUsage {
        usage
    }
}
```

- [ ] **Step 2: Create minimal app source files**

Create `apps/macos-native/AIUsageTracker/App/AIUsageTrackerApp.swift`:

```swift
import SwiftUI
import UsageCore

@main
struct AIUsageTrackerApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @State private var appStore = AppStore.live()

    var body: some Scene {
        MenuBarExtra {
            MenuBarStatusView(store: appStore)
        } label: {
            Text(appStore.menuBarTitle)
        }
        .menuBarExtraStyle(.menu)

        Window("Dashboard", id: "dashboard") {
            DashboardWindowView(store: appStore)
        }

        Settings {
            NativeSettingsView(store: appStore)
        }
    }
}
```

Create `apps/macos-native/AIUsageTracker/App/AppDelegate.swift`:

```swift
import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
    }
}
```

Create `apps/macos-native/AIUsageTracker/Stores/AppStore.swift`:

```swift
import Foundation
import Observation
import UsageCore

@Observable
@MainActor
final class AppStore {
    private let refreshCoordinator: RefreshCoordinator

    var snapshot: UsageSnapshot?
    var isRefreshing = false
    var showOffline = false

    init(refreshCoordinator: RefreshCoordinator) {
        self.refreshCoordinator = refreshCoordinator
    }

    static func live() -> AppStore {
        let apiKey = ProcessInfo.processInfo.environment["ZAI_API_KEY"] ?? ""
        let client = ZaiProviderClient(apiKey: apiKey, label: nil)
        return AppStore(refreshCoordinator: RefreshCoordinator(providerClients: [client]))
    }

    var menuBarTitle: String {
        guard let usage = snapshot?.services.first(where: { $0.provider == .zai }) else {
            return "AI Usage"
        }
        return UsageProjection.menuBarTitle(for: usage)
    }

    func refresh() async {
        if isRefreshing { return }
        isRefreshing = true
        snapshot = await refreshCoordinator.refreshAll()
        isRefreshing = false
    }
}
```

Create `apps/macos-native/AIUsageTracker/Resources/Info.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>$(DEVELOPMENT_LANGUAGE)</string>
  <key>CFBundleExecutable</key>
  <string>$(EXECUTABLE_NAME)</string>
  <key>CFBundleIdentifier</key>
  <string>$(PRODUCT_BUNDLE_IDENTIFIER)</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>AI Usage Tracker Native</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>14.0</string>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
</dict>
</plist>
```

Create `apps/macos-native/AIUsageTracker/AIUsageTracker.entitlements`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict/>
</plist>
```

- [ ] **Step 3: Add SwiftUI scene files**

Create `apps/macos-native/AIUsageTracker/Scenes/MenuBarStatusView.swift`:

```swift
import SwiftUI
import UsageCore

struct MenuBarStatusView: View {
    @Bindable var store: AppStore
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        if let usage = store.snapshot?.services.first(where: { $0.provider == .zai }) {
            Text(UsageProjection.menuBarTitle(for: usage))
        } else {
            Text("No usage loaded")
        }

        Divider()

        Button("Refresh Now") {
            Task { await store.refresh() }
        }
        .disabled(store.isRefreshing)

        Button("Show Dashboard") {
            NSApp.setActivationPolicy(.regular)
            NSApp.activate(ignoringOtherApps: true)
            openWindow(id: "dashboard")
        }

        Button("Settings") {
            NSApp.setActivationPolicy(.regular)
            NSApp.activate(ignoringOtherApps: true)
            openSettings()
        }

        Divider()

        Button("Quit") {
            NSApp.terminate(nil)
        }
    }
}
```

Create `apps/macos-native/AIUsageTracker/Scenes/DashboardWindowView.swift`:

```swift
import SwiftUI

struct DashboardWindowView: View {
    @Bindable var store: AppStore

    var body: some View {
        DashboardView(store: store)
            .frame(minWidth: 760, minHeight: 520)
    }
}
```

Create `apps/macos-native/AIUsageTracker/Features/Settings/NativeSettingsView.swift`:

```swift
import SwiftUI

struct NativeSettingsView: View {
    @Bindable var store: AppStore

    var body: some View {
        Form {
            Toggle("Show offline accounts", isOn: $store.showOffline)
        }
        .padding()
        .frame(width: 420)
    }
}
```

- [ ] **Step 4: Add app UI files**

Create `apps/macos-native/AIUsageTracker/Features/Dashboard/DashboardView.swift`:

```swift
import SwiftUI
import UsageCore

struct DashboardView: View {
    @Bindable var store: AppStore

    var body: some View {
        NavigationSplitView {
            AccountListView(store: store)
        } detail: {
            AccountDetailView(store: store)
        }
        .toolbar {
            Button("Refresh") {
                Task { await store.refresh() }
            }
            .disabled(store.isRefreshing)
        }
        .task {
            if store.snapshot == nil {
                await store.refresh()
            }
        }
    }
}
```

Create `apps/macos-native/AIUsageTracker/Features/Dashboard/AccountListView.swift`:

```swift
import SwiftUI
import UsageCore

struct AccountListView: View {
    @Bindable var store: AppStore

    var body: some View {
        List {
            if let services = store.snapshot?.services, !services.isEmpty {
                ForEach(services, id: \.id) { usage in
                    VStack(alignment: .leading, spacing: 4) {
                        Text(UsageProjection.providerDisplayName(usage.provider))
                            .font(.headline)
                        Text(UsageProjection.subtitle(for: usage))
                            .foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 4)
                }
            } else {
                Text(store.isRefreshing ? "Refreshing..." : "No accounts")
                    .foregroundStyle(.secondary)
            }
        }
        .navigationTitle("Accounts")
    }
}
```

Create `apps/macos-native/AIUsageTracker/Features/Dashboard/AccountDetailView.swift`:

```swift
import SwiftUI
import UsageCore

struct AccountDetailView: View {
    @Bindable var store: AppStore

    var body: some View {
        if let usage = store.snapshot?.services.first {
            VStack(alignment: .leading, spacing: 16) {
                Text(UsageProjection.providerDisplayName(usage.provider))
                    .font(.largeTitle.bold())
                ForEach(usage.windows, id: \.label) { window in
                    VStack(alignment: .leading) {
                        HStack {
                            Text(window.label)
                            Spacer()
                            if let remaining = UsageProjection.remainingPercent(for: usage) {
                                Text("\(remaining)% remaining")
                                    .foregroundStyle(.secondary)
                            }
                        }
                        ProgressView(value: window.usedPercent ?? 0, total: 100)
                    }
                }
                Spacer()
            }
            .padding()
        } else {
            ContentUnavailableView("No Usage", systemImage: "chart.bar", description: Text("Refresh z.ai usage to populate the dashboard."))
        }
    }
}
```

Create `apps/macos-native/AIUsageTracker/Features/AddAccount/AddZaiAccountSheet.swift`:

```swift
import SwiftUI

struct AddZaiAccountSheet: View {
    @State private var label = ""
    @State private var apiKey = ""

    var body: some View {
        Form {
            TextField("Label", text: $label)
            SecureField("z.ai API key", text: $apiKey)
            Button("Save") {
            }
            .disabled(apiKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
        .padding()
        .frame(width: 420)
    }
}
```

- [ ] **Step 5: Create the Xcode project**

Create `apps/macos-native/AIUsageTracker.xcodeproj/project.pbxproj` as a valid Xcode 27 macOS SwiftUI project. Generate the project in whichever local Xcode-supported way is fastest, then commit the deterministic `project.pbxproj`; the committed result must define exactly this native app surface and must pass the `xcodebuild -list` command below before moving on:

- app target `AIUsageTracker`
- unit test target `AIUsageTrackerTests`
- product bundle identifier `com.aiusage.tracker.native`
- macOS deployment target 14.0 or newer
- local Swift package dependency at `Packages/UsageCore`
- app source files from `apps/macos-native/AIUsageTracker/**`
- test source file from `apps/macos-native/AIUsageTrackerTests/AppStoreTests.swift`
- Info.plist path `AIUsageTracker/Resources/Info.plist`
- entitlements path `AIUsageTracker/AIUsageTracker.entitlements`

After creating the project, run:

```bash
xcodebuild -list -project apps/macos-native/AIUsageTracker.xcodeproj
```

Expected: output lists scheme `AIUsageTracker`.

- [ ] **Step 6: Add native run script**

Create `scripts/macos-native-run.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT="$ROOT/apps/macos-native/AIUsageTracker.xcodeproj"
SCHEME="AIUsageTracker"
DERIVED="$ROOT/.build/xcode/macos-native"
APP="$DERIVED/Build/Products/Debug/AI Usage Tracker Native.app"
PROCESS="AI Usage Tracker Native"

if pgrep -x "$PROCESS" >/dev/null 2>&1; then
  pkill -x "$PROCESS" || true
fi

xcodebuild \
  -project "$PROJECT" \
  -scheme "$SCHEME" \
  -configuration Debug \
  -derivedDataPath "$DERIVED" \
  build

/usr/bin/open -n "$APP"

if [[ "${1:-}" == "--verify" ]]; then
  for _ in {1..30}; do
    if pgrep -x "$PROCESS" >/dev/null 2>&1; then
      echo "Native app is running: $PROCESS"
      exit 0
    fi
    sleep 0.5
  done
  echo "Native app did not start: $PROCESS" >&2
  exit 1
fi
```

Run:

```bash
chmod +x scripts/macos-native-run.sh
```

- [ ] **Step 7: Update `.gitignore` for native build outputs**

Append to `.gitignore`:

```gitignore

# Native macOS build output
.build/xcode/
apps/macos-native/DerivedData/
```

- [ ] **Step 8: Run tests and verify GREEN**

Run:

```bash
swift test --package-path apps/macos-native/Packages/UsageCore
xcodebuild test -project apps/macos-native/AIUsageTracker.xcodeproj -scheme AIUsageTracker -destination 'platform=macOS' -derivedDataPath .build/xcode/macos-native
scripts/macos-native-run.sh --verify
```

Expected: SwiftPM tests pass, Xcode tests pass, and the native app launches.

- [ ] **Step 9: Commit Task 6**

```bash
git add .gitignore apps/macos-native scripts/macos-native-run.sh
git commit -m "feat: scaffold native macOS app shell"
```

## Task 7: Native z.ai Account Sheet Persistence

**Files:**
- Modify: `apps/macos-native/AIUsageTracker/Stores/AppStore.swift`
- Modify: `apps/macos-native/AIUsageTracker/Features/AddAccount/AddZaiAccountSheet.swift`
- Modify: `apps/macos-native/AIUsageTracker/Features/Dashboard/DashboardView.swift`
- Modify: `apps/macos-native/AIUsageTrackerTests/AppStoreTests.swift`

- [ ] **Step 1: Add failing app store account test**

Append to `AppStoreTests.swift`:

```swift
func testSaveZaiAccountWritesNativeAccountAndRefreshesRegistry() async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: "NativeAppStoreTests-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    let accountStore = AccountStore(rootDirectory: root)
    let store = AppStore(
        refreshCoordinator: RefreshCoordinator(providerClients: [], clock: { 1_800_000_000 }),
        accountStore: accountStore
    )

    try await store.saveZaiAccount(label: "Work", apiKey: "zai-secret-987")

    let accounts = try await accountStore.load()
    XCTAssertEqual(accounts.count, 1)
    XCTAssertEqual(accounts[0].provider, .zai)
    XCTAssertEqual(accounts[0].label, "Work")
    XCTAssertEqual(accounts[0].credential, "zai-secret-987")
}
```

- [ ] **Step 2: Run test and verify RED**

Run:

```bash
xcodebuild test -project apps/macos-native/AIUsageTracker.xcodeproj -scheme AIUsageTracker -destination 'platform=macOS' -derivedDataPath .build/xcode/macos-native -only-testing:AIUsageTrackerTests/AppStoreTests/testSaveZaiAccountWritesNativeAccountAndRefreshesRegistry
```

Expected: FAIL to compile with no `AppStore.saveZaiAccount`.

- [ ] **Step 3: Implement account store injection and save method**

Modify `AppStore.swift`:

```swift
import Foundation
import Observation
import UsageCore

@Observable
@MainActor
final class AppStore {
    private var refreshCoordinator: RefreshCoordinator
    private let accountStore: AccountStore

    var snapshot: UsageSnapshot?
    var isRefreshing = false
    var showOffline = false
    var isAddingZaiAccount = false

    init(refreshCoordinator: RefreshCoordinator, accountStore: AccountStore = AccountStore(rootDirectory: AppStore.defaultStorageRoot())) {
        self.refreshCoordinator = refreshCoordinator
        self.accountStore = accountStore
    }

    static func live() -> AppStore {
        let root = defaultStorageRoot()
        let accountStore = AccountStore(rootDirectory: root)
        let apiKey = ProcessInfo.processInfo.environment["ZAI_API_KEY"] ?? ""
        let client = ZaiProviderClient(apiKey: apiKey, label: nil)
        return AppStore(refreshCoordinator: RefreshCoordinator(providerClients: [client]), accountStore: accountStore)
    }

    static func defaultStorageRoot() -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        return base.appending(path: "com.aiusage.tracker.native", directoryHint: .isDirectory)
    }

    var menuBarTitle: String {
        guard let usage = snapshot?.services.first(where: { $0.provider == .zai }) else {
            return "AI Usage"
        }
        return UsageProjection.menuBarTitle(for: usage)
    }

    func refresh() async {
        if isRefreshing { return }
        isRefreshing = true
        snapshot = await refreshCoordinator.refreshAll()
        isRefreshing = false
    }

    func saveZaiAccount(label: String, apiKey: String) async throws {
        let trimmedKey = apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedKey.isEmpty else {
            throw ProviderError.notLoggedIn("z.ai API key is blank")
        }
        var accounts = try await accountStore.load()
        let account = StoredAccount(
            id: UUID().uuidString,
            provider: .zai,
            label: label.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "z.ai" : label,
            credential: trimmedKey
        )
        accounts.append(account)
        try await accountStore.save(accounts)
        refreshCoordinator = RefreshCoordinator(providerClients: [
            ZaiProviderClient(apiKey: trimmedKey, label: account.label)
        ])
    }
}
```

- [ ] **Step 4: Wire Add Account sheet**

Modify `DashboardView.swift` toolbar:

```swift
.toolbar {
    Button("Add z.ai") {
        store.isAddingZaiAccount = true
    }
    Button("Refresh") {
        Task { await store.refresh() }
    }
    .disabled(store.isRefreshing)
}
.sheet(isPresented: $store.isAddingZaiAccount) {
    AddZaiAccountSheet(store: store)
}
```

Modify `AddZaiAccountSheet.swift`:

```swift
import SwiftUI

struct AddZaiAccountSheet: View {
    @Bindable var store: AppStore
    @Environment(\.dismiss) private var dismiss
    @State private var label = ""
    @State private var apiKey = ""
    @State private var errorMessage: String?

    var body: some View {
        Form {
            TextField("Label", text: $label)
            SecureField("z.ai API key", text: $apiKey)
            if let errorMessage {
                Text(errorMessage)
                    .foregroundStyle(.red)
            }
            HStack {
                Button("Cancel") {
                    dismiss()
                }
                Spacer()
                Button("Save") {
                    Task {
                        do {
                            try await store.saveZaiAccount(label: label, apiKey: apiKey)
                            dismiss()
                        } catch {
                            errorMessage = String(describing: error)
                        }
                    }
                }
                .disabled(apiKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding()
        .frame(width: 420)
    }
}
```

- [ ] **Step 5: Run tests and verify GREEN**

Run:

```bash
xcodebuild test -project apps/macos-native/AIUsageTracker.xcodeproj -scheme AIUsageTracker -destination 'platform=macOS' -derivedDataPath .build/xcode/macos-native
scripts/macos-native-run.sh --verify
```

Expected: PASS and app launches.

- [ ] **Step 6: Commit Task 7**

```bash
git add apps/macos-native
git commit -m "feat: persist native z.ai accounts"
```

## Task 8: Final Verification And Baseline Check

**Files:** none

- [ ] **Step 1: Run existing Tauri baseline**

Run:

```bash
pnpm verify:runtime
```

Expected: PASS. If this fails because of pre-existing environment issues, record the smallest error snippet in the final task report and do not modify Tauri code in this native slice.

- [ ] **Step 2: Run native core tests**

Run:

```bash
swift test --package-path apps/macos-native/Packages/UsageCore
```

Expected: PASS.

- [ ] **Step 3: Run native app tests**

Run:

```bash
xcodebuild test -project apps/macos-native/AIUsageTracker.xcodeproj -scheme AIUsageTracker -destination 'platform=macOS' -derivedDataPath .build/xcode/macos-native
```

Expected: PASS.

- [ ] **Step 4: Run native launch verification**

Run:

```bash
scripts/macos-native-run.sh --verify
```

Expected: PASS with `Native app is running: AI Usage Tracker Native`.

- [ ] **Step 5: Confirm Tauri files were not changed**

Run:

```bash
git diff --name-only HEAD -- src src-tauri package.json scripts/tauri.mjs
```

Expected: no output.

- [ ] **Step 6: Commit final verification note if plan was updated**

Only if this plan file was updated during implementation:

```bash
git add docs/superpowers/plans/2026-06-23-native-swift-zai-slice.md
git commit -m "docs: update native Swift z.ai implementation notes"
```

## Execution Notes For Subagents

- Implement tasks sequentially. Do not dispatch multiple workers to edit the same native app tree at the same time.
- Each implementation worker must use TDD unless the task is explicitly scaffold generation.
- Each task should be followed by spec compliance review and code quality review.
- Do not edit `src/`, `src-tauri/`, `package.json`, or `scripts/tauri.mjs` in this plan.
- Keep `.superpowers/` ignored and uncommitted.
