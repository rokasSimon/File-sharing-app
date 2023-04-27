import React from "react";
import { listen } from "@tauri-apps/api/event";
import { GetShareDirectories, invokeNetworkCommand } from "./networkCommands";
import { v4, validate as validateUuid } from "uuid";

type SerialisedShareDirectory = {
  signature: ShareDirectorySignature,
  shared_files: any
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
  directoryPath: {
    localPath: string;
  };
  ownedPeers: Array<PeerId>;
  size: number,
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

    const startListenUpdate = async () => {
      const _ = await listen<Array<SerialisedShareDirectory>>(
        "UpdateShareDirectories",
        (event) => {
          console.log(
            `Received ${JSON.stringify(
              event.payload
            )} from rust in UpdateShareDirectories`
          );
          const input = event.payload;
          const dirs = input.map((dir) => {
            const fileMap = new Map<string, SharedFile>();

            Object.keys(dir.shared_files).forEach((key) => {
              if (validateUuid(key)) {
                fileMap.set(key, (dir.shared_files[key]) as SharedFile);
              }
            });

            const directory: ShareDirectory = {
              signature: dir.signature,
              shared_files: fileMap,
            };

            return directory;
          });

          console.log(
            `Setting to ${JSON.stringify(
              dirs
            )}`
          );

          setDirectories(dirs);
        }
      );
    };

    const startListenFileAdded = async () => {
      const _ = await listen<AddedFiles>(
        "AddedFiles",
        (event) => {
          const input = event.payload;
          const directory = directoriesRef.current.find((dir) => {
            console.log(`${dir.signature.identifier} === ${input.directoryIdentifier}`);
            
            return dir.signature.identifier === input.directoryIdentifier;
          });

          if (directory) {
            for (const file of input.sharedFiles) {
              directory.shared_files.set(file.identifier, file);
            }
          }

          const dirs = [
            ...directoriesRef.current
          ];

          setDirectories(dirs);
        }
      );
    };

    const loadDirectories = async () => {
      const request: GetShareDirectories = {
        getAllShareDirectoryData: false,
      };

      await invokeNetworkCommand(request)
    };

    startListenUpdate();
    startListenNewDir();
    startListenFileAdded();
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
