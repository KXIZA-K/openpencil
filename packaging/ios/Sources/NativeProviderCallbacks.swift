import Foundation

/// Result of one native provider-SDK authorization attempt (Douyin /
/// Alipay): an auth code for the SSO native-login exchange, a user cancel
/// that simply returns to the login screen, or a failure surfaced inline.
enum NativeSignInOutcome {
    case authorized(authCode: String)
    case canceled
    case failed
}

/// Routes scheme callbacks from native provider SDK flows (Douyin / Alipay)
/// back into their SDKs. Wired to the SwiftUI scene's `onOpenURL`.
enum NativeProviderCallbacks {
    @discardableResult
    static func handle(_ url: URL) -> Bool {
        if DouyinNativeSignIn.handleOpenURL(url) { return true }
        return AlipayNativeSignIn.handleOpenURL(url)
    }
}
