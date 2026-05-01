settingsEvaluated {
    // Resolution mirrors cache.nu local-root:
    //   1. $BAYT_CACHE_DIR (explicit override)
    //   2. $XDG_CACHE_HOME/bayt (XDG Base Directory spec, *nix idiomatic)
    //   3. $LOCALAPPDATA/bayt (Windows-idiomatic, undefined on *nix)
    //   4. ~/.cache/bayt (XDG fallback — also lands at /root/.cache/bayt
    //      inside the BuildKit cache mount because user.home is /root)
    val cacheDir = System.getenv("BAYT_CACHE_DIR")
        ?: System.getenv("XDG_CACHE_HOME")?.let { "$it/bayt" }
        ?: System.getenv("LOCALAPPDATA")?.let { "$it/bayt" }
        ?: "${System.getProperty("user.home")}/.cache/bayt"
    buildCache {
        local {
            directory = file("$cacheDir/gradle")
        }
    }
}
