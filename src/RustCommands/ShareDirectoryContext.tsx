import React from "react";
import { listen } from "@tauri-apps/api/event";
import { GetShareDirectories, invokeNetworkCommand } from "./networkCommands";
import { v4, validate as validateUuid } from "uuid";

type SerialisedShareDirectory = {
  signature: ShareDirectorySignature;
  shared_files: any;
};

type ShareDirectories = Array<ShareDirectory>;

type ShareDirectory = {
  signature: ShareDirectorySignature;
  shared_files: Map<string, SharedFile>;
};

type ShareDirectorySignature = {
  name: string;
  identifier: string;
  lastTransactionId: string;
  sharedPeers: Array<PeerId>;
};

type SharedFile = {
  name: string;
  identifier: string;
  contentHash: number;
  lastModified: string;
  contentLocation: {
    localPath: string;
  } | undefined;
  ownedPeers: Array<PeerId>;
  size: number;
};

type PeerId = {
  hostname: string;
  uuid: string;
};

type AddedFiles = {
  directoryIdentifier: string;
  sharedFiles: Array<SharedFile>;
};

const initialState: ShareDirectories = [];
const ShareDirectoryContext =
  React.createContext<ShareDirectories>(initialState);

function ShareDirectoryProvider({ children }: any) {
  const [directories, setDirectories] = React.useState(initialState);
  const directoriesRef = React.useRef(directories);
  const loaded = React.useRef(false);

  React.useEffect(() => {
    directoriesRef.current = directories;
  }, [directories]);

  React.useEffect(() => {
    if (loaded.current) return;

    const startListenNewDir = async () => {
      const _ = await listen<ShareDirectorySignature>(
        "NewShareDirectory",
        (event) => {
          const input = event.payload;
          const newDirs: ShareDirectories = [
            ...directoriesRef.current,
            { signature: input, shared_files: new Map() },
          ];

          setDirectories(newDirs);
        }
      );
    };

    const startListenSync = async () => {
      const _ = await listen<Array<SerialisedShareDirectory>>(
        "UpdateShareDirectories",
        (event) => {
          const input = event.payload;
          const dirs = input.map((dir) => {
            const fileMap = new Map<string, SharedFile>();

            Object.keys(dir.shared_files).forEach((key) => {
              if (validateUuid(key)) {
                fileMap.set(key, dir.shared_files[key] as SharedFile);
              }
            });

            const directory: ShareDirectory = {
              signature: dir.signature,
              shared_files: fileMap,
            };

            return directory;
          });

          setDirectories(dirs);
        }
      );
    };

    const startListenUpdateDirectory = async () => {
      const _ = await listen<SerialisedShareDirectory>("UpdateDirectory", (event) => {
        const input = event.payload;
        const fileMap = new Map<string, SharedFile>();

        Object.keys(input.shared_files).forEach((key) => {
          if (validateUuid(key)) {
            fileMap.set(key, (input.shared_files[key]) as SharedFile);
          }
        });

        const newDirectory: ShareDirectory = {
          signature: input.signature,
          shared_files: fileMap
        };
        let updatedDirectories = [ newDirectory ];

        for (const directory of directoriesRef.current) {
          if (directory.signature.identifier !== input.signature.identifier) {
            updatedDirectories.push(directory);
          }
        }

        const sortedDirectories = updatedDirectories.sort((a, b) => {
          if (a.signature.identifier < b.signature.identifier) {
            return -1;
          } else if (a.signature.identifier > b.signature.identifier) {
            return 1;
          }

          return 0;
        });

        setDirectories(sortedDirectories);
      });
    };

    const loadDirectories = async () => {
      const request: GetShareDirectories = {
        getAllShareDirectoryData: false,
      };

      await invokeNetworkCommand(request);
    };

    startListenSync();
    startListenNewDir();
    startListenUpdateDirectory();
    loadDirectories();

    loaded.current = true;
  }, []);

  return (
    <ShareDirectoryContext.Provider value={directories}>
      {children}
    </ShareDirectoryContext.Provider>
  );
}

export { ShareDirectoryProvider, ShareDirectoryContext };
export type { SharedFile, ShareDirectory, ShareDirectorySignature, PeerId };
