import AuthenticationServices
import Foundation

/// GitHub OAuth flow via ASWebAuthenticationSession.
///
/// Usage from Rust (via FFI):
///   1. Call `github_oauth_start(clientId, callbackScheme)` — opens browser
///   2. Call `github_oauth_get_token()` — returns stored token (or null)
///   3. Call `github_oauth_clear_token()` — removes stored token

private let keychainKey = "github_oauth_token"

// MARK: - C-callable FFI functions

/// Start the OAuth flow. Returns immediately; token is stored asynchronously.
/// `clientId` and `callbackScheme` are null-terminated C strings.
@_cdecl("github_oauth_start")
func githubOAuthStart(clientId: UnsafePointer<CChar>, callbackScheme: UnsafePointer<CChar>) {
    let clientIdStr = String(cString: clientId)
    let schemeStr = String(cString: callbackScheme)

    DispatchQueue.main.async {
        GitHubOAuth.authenticate(clientId: clientIdStr, callbackScheme: schemeStr)
    }
}

/// Returns the stored GitHub token as a C string, or null if not authenticated.
/// The caller must free the returned pointer with `free()`.
@_cdecl("github_oauth_get_token")
func githubOAuthGetToken() -> UnsafeMutablePointer<CChar>? {
    guard let token = KeychainHelper.load(key: keychainKey) else { return nil }
    return strdup(token)
}

/// Clear the stored token (sign out).
@_cdecl("github_oauth_clear_token")
func githubOAuthClearToken() {
    KeychainHelper.delete(key: keychainKey)
}

// MARK: - OAuth Implementation

enum GitHubOAuth {
    /// Trigger the GitHub OAuth flow using ASWebAuthenticationSession.
    static func authenticate(clientId: String, callbackScheme: String) {
        let scope = "repo"
        let state = UUID().uuidString
        let authURL = URL(string:
            "https://github.com/login/oauth/authorize" +
            "?client_id=\(clientId)" +
            "&scope=\(scope)" +
            "&state=\(state)" +
            "&redirect_uri=\(callbackScheme)://callback"
        )!

        let session = ASWebAuthenticationSession(
            url: authURL,
            callbackURLScheme: callbackScheme
        ) { callbackURL, error in
            guard error == nil,
                  let url = callbackURL,
                  let components = URLComponents(url: url, resolvingAgainstBaseURL: false),
                  let code = components.queryItems?.first(where: { $0.name == "code" })?.value
            else {
                return
            }
            // Exchange authorization code for access token
            exchangeCodeForToken(code: code, clientId: clientId)
        }

        // On iOS, ASWebAuthenticationSession handles its own presentation
        session.prefersEphemeralWebBrowserSession = false
        session.start()
    }

    /// Exchange the authorization code for an access token via GitHub's token endpoint.
    /// Note: This requires a client secret, which should be handled server-side in production.
    /// For development, we use the device flow or a thin proxy. Here we store the code
    /// directly and let the Rust side handle the exchange if needed.
    private static func exchangeCodeForToken(code: String, clientId: String) {
        // For now, store the authorization code. The Rust side will exchange it
        // via a server-side proxy or the user provides a PAT.
        // In a full implementation, you'd POST to a backend that holds the client secret.
        //
        // Alternatively, if using GitHub's Device Flow (no client secret needed):
        // The token is obtained directly. For OAuth Apps with a proxy:
        let _ = KeychainHelper.save(key: keychainKey, value: code)
    }
}
