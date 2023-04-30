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

interface DownloadFile extends BackendCommand {
    downloadFile: {
        directory_identifier: string,
        file_identifier: string,
    }
}

interface DeleteFile extends BackendCommand {
    deleteFile: {
        directory_identifier: string,
        file_identifier: string,
    }
}

interface CancelDownload extends BackendCommand {
    cancelDownload: {
        download_identifier: string,
        peer: PeerId,
    }
}

async function invokeBackendCommand(command: BackendCommand): Promise<any> {
    console.log(JSON.stringify(command));
    const result = await invoke('network_command', {
        message: command
    });

    return result;
}

export type { CreateShareDirectory, GetShareDirectories, AddFiles, ShareDirectoryToPeers, DownloadFile, DeleteFile, CancelDownload };
export { invokeBackendCommand as invokeNetworkCommand };