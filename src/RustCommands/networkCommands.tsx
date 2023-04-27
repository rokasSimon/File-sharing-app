import { invoke } from "@tauri-apps/api/tauri";
import { PeerId } from "./ShareDirectoryContext";

class BackendCommand {

}

interface CreateShareDirectory extends BackendCommand {
    createShareDirectory: string
}

interface GetShareDirectories extends BackendCommand {
    getAllShareDirectoryData: boolean
}

interface AddFiles extends BackendCommand {
    addFiles: {
        directory_identifier: string
        file_paths: string[]
    }
}

interface ShareDirectoryToPeers extends BackendCommand {
    shareDirectoryToPeers: {
        directory_identifier: string,
        peers: Array<PeerId>,
    },
}

async function invokeBackendCommand(command: BackendCommand): Promise<any> {
    console.log(JSON.stringify(command));
    const result = await invoke('network_command', {
        message: command
    });

    return result;
}

export type { CreateShareDirectory, GetShareDirectories, AddFiles, ShareDirectoryToPeers };
export { invokeBackendCommand as invokeNetworkCommand };