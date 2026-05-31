# rmapi overlay — apply ddvk/rmapi's v4 sync-schema fix to nixpkgs' rmapi.
#
# Background: reMarkable rolled out a v4 sync schema on 2026-05-18 that rejects
# rmapi's `rm-filename` HTTP header with HTTP 400 when the value has no file
# extension. nixpkgs' rmapi (0.0.32) sends bare UUIDs / literal "roothash" and
# is rejected on every blob fetch/put. The fix merged upstream 2026-05-20 (PR
# #63) but is not in any release yet, and nixpkgs has not bumped.
#
# PR #65 adds an `ensureExtension()` helper at the BlobStorage boundary that
# defaults missing-extension filenames to `.docSchema`. 0.0.32 already ships
# `put --content-only`, so this overlay yields a fully working rmapi.
#
# REMOVE this overlay once nixpkgs ships rmapi >= the release containing the v4
# fix (>= 0.0.34, whenever ddvk tags it).
self: super: {
  rmapi = super.rmapi.overrideAttrs (old: {
    patches =
      (old.patches or [ ])
      ++ [
        (super.fetchpatch {
          name = "pr-65-ensure-extension-on-rm-filename-header.patch";
          url = "https://github.com/ddvk/rmapi/pull/65.patch";
          hash = "sha256-APwjyV/CV3Xac+DrlrptjYRBo8B1AtjU2ehg4/lJfbg=";
        })
      ];
  });
}
