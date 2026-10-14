# Third-party components

The root MIT license covers original lttl code and documentation. It does not
relicense dependencies, vendor runtimes, trademarks or third-party material in
the historical research evidence.

Rust dependencies retain their respective licenses. The Windows packaging
script collects their supplied license/notice files and a package/license
inventory into the installer's `licenses/rust/` directory, including nested
native-code notices. Cargo.lock pins dependency versions.

The optional NVIDIA component includes nvCOMP and CUDA runtime binaries under
their NVIDIA terms, with `LICENSE-nvcomp.txt`, `LICENSE-cuda-runtime.txt` and
`NOTICE-nvcomp.txt`. These components are for use by lttl on systems with NVIDIA
GPUs. They are not covered by lttl's MIT license. No NVIDIA compiler, driver or
complete SDK is installed by the lttl installer.

Microsoft Visual C++ runtime DLLs are included app-locally from the Visual
Studio redistributable directory. They remain Microsoft components subject to
their applicable terms; they are not modified or installed system-wide.

NSIS's installer engine retains its license, included under `licenses/nsis/`.
Compiler binaries, downloaded SDKs, sample corpora and third-party application
executables are excluded from the source repository. Historical REA evidence
is retained as research material, not as the source of lttl's compression codecs.
