# Details

Date : 2023-05-01 15:00:22

Directory e:\\DEV\\Rust\\file-sharing-app

Total : 44 files,  34979 codes, 74 comments, 894 blanks, all 35947 lines

[Summary](results.md) / Details / [Diff Summary](diff.md) / [Diff Details](diff-details.md)

## Files
| filename | language | code | comment | blank | total |
| :--- | :--- | ---: | ---: | ---: | ---: |
| [README.md](/README.md) | Markdown | 26 | 0 | 21 | 47 |
| [launch.ps1](/launch.ps1) | PowerShell | 2 | 0 | 0 | 2 |
| [package-lock.json](/package-lock.json) | JSON | 30,267 | 0 | 1 | 30,268 |
| [package.json](/package.json) | JSON | 59 | 0 | 1 | 60 |
| [public/index.html](/public/index.html) | HTML | 20 | 23 | 1 | 44 |
| [public/manifest.json](/public/manifest.json) | JSON | 25 | 0 | 1 | 26 |
| [src-tauri/Cargo.toml](/src-tauri/Cargo.toml) | TOML | 42 | 5 | 7 | 54 |
| [src-tauri/build.rs](/src-tauri/build.rs) | Rust | 6 | 0 | 4 | 10 |
| [src-tauri/src/config.rs](/src-tauri/src/config.rs) | Rust | 142 | 0 | 39 | 181 |
| [src-tauri/src/data.rs](/src-tauri/src/data.rs) | Rust | 101 | 0 | 23 | 124 |
| [src-tauri/src/main.rs](/src-tauri/src/main.rs) | Rust | 142 | 0 | 25 | 167 |
| [src-tauri/src/network.rs](/src-tauri/src/network.rs) | Rust | 45 | 0 | 10 | 55 |
| [src-tauri/src/network/client_handle.rs](/src-tauri/src/network/client_handle.rs) | Rust | 802 | 1 | 163 | 966 |
| [src-tauri/src/network/codec.rs](/src-tauri/src/network/codec.rs) | Rust | 126 | 2 | 38 | 166 |
| [src-tauri/src/network/codec/protobuf_map.rs](/src-tauri/src/network/codec/protobuf_map.rs) | Rust | 120 | 1 | 22 | 143 |
| [src-tauri/src/network/codec/tcpMessages.proto](/src-tauri/src/network/codec/tcpMessages.proto) | Protocol Buffers | 115 | 0 | 23 | 138 |
| [src-tauri/src/network/mdns.rs](/src-tauri/src/network/mdns.rs) | Rust | 152 | 0 | 30 | 182 |
| [src-tauri/src/network/server_handle.rs](/src-tauri/src/network/server_handle.rs) | Rust | 868 | 0 | 156 | 1,024 |
| [src-tauri/src/network/tcp_listener.rs](/src-tauri/src/network/tcp_listener.rs) | Rust | 19 | 0 | 7 | 26 |
| [src-tauri/src/peer_id.rs](/src-tauri/src/peer_id.rs) | Rust | 44 | 0 | 12 | 56 |
| [src-tauri/src/window.rs](/src-tauri/src/window.rs) | Rust | 63 | 0 | 18 | 81 |
| [src-tauri/tauri.conf.json](/src-tauri/tauri.conf.json) | JSON | 110 | 0 | 0 | 110 |
| [src/App.css](/src/App.css) | CSS | 49 | 0 | 13 | 62 |
| [src/App.test.tsx](/src/App.test.tsx) | TypeScript JSX | 8 | 0 | 2 | 10 |
| [src/App.tsx](/src/App.tsx) | TypeScript JSX | 155 | 0 | 27 | 182 |
| [src/Components/DirectoryDetails.tsx](/src/Components/DirectoryDetails.tsx) | TypeScript JSX | 299 | 0 | 34 | 333 |
| [src/Components/Menu.css](/src/Components/Menu.css) | CSS | 15 | 0 | 2 | 17 |
| [src/Components/Menu.tsx](/src/Components/Menu.tsx) | TypeScript JSX | 47 | 0 | 6 | 53 |
| [src/Components/ResizableBox.css](/src/Components/ResizableBox.css) | CSS | 39 | 0 | 6 | 45 |
| [src/Components/ResizableBox.tsx](/src/Components/ResizableBox.tsx) | TypeScript JSX | 51 | 0 | 8 | 59 |
| [src/RustCommands/ConnectedDevicesContext.tsx](/src/RustCommands/ConnectedDevicesContext.tsx) | TypeScript JSX | 45 | 0 | 14 | 59 |
| [src/RustCommands/DownloadsManager.tsx](/src/RustCommands/DownloadsManager.tsx) | TypeScript JSX | 144 | 0 | 31 | 175 |
| [src/RustCommands/ShareDirectoryContext.tsx](/src/RustCommands/ShareDirectoryContext.tsx) | TypeScript JSX | 136 | 0 | 33 | 169 |
| [src/RustCommands/networkCommands.tsx](/src/RustCommands/networkCommands.tsx) | TypeScript JSX | 53 | 0 | 12 | 65 |
| [src/index.css](/src/index.css) | CSS | 28 | 0 | 6 | 34 |
| [src/index.tsx](/src/index.tsx) | TypeScript JSX | 34 | 3 | 7 | 44 |
| [src/logo.svg](/src/logo.svg) | XML | 1 | 0 | 0 | 1 |
| [src/pages/Directories.css](/src/pages/Directories.css) | CSS | 48 | 0 | 10 | 58 |
| [src/pages/Directories.tsx](/src/pages/Directories.tsx) | TypeScript JSX | 386 | 0 | 53 | 439 |
| [src/pages/Settings.tsx](/src/pages/Settings.tsx) | TypeScript JSX | 105 | 34 | 22 | 161 |
| [src/react-app-env.d.ts](/src/react-app-env.d.ts) | TypeScript | 0 | 1 | 1 | 2 |
| [src/reportWebVitals.ts](/src/reportWebVitals.ts) | TypeScript | 13 | 0 | 3 | 16 |
| [src/setupTests.ts](/src/setupTests.ts) | TypeScript | 1 | 4 | 1 | 6 |
| [tsconfig.json](/tsconfig.json) | JSON with Comments | 26 | 0 | 1 | 27 |

[Summary](results.md) / Details / [Diff Summary](diff.md) / [Diff Details](diff-details.md)