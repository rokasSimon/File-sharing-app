import {
  Backdrop,
  Box,
  Breadcrumbs,
  Button,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
  Divider,
  IconButton,
  Link,
  List,
  ListItem,
  ListItemText,
  ListSubheader,
  Paper,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableFooter,
  TableHead,
  TableRow,
  Typography,
} from "@mui/material";
import InfoRoundedIcon from "@mui/icons-material/InfoRounded";
import NavigateNextRoundedIcon from "@mui/icons-material/NavigateNextRounded";
import FolderIcon from "@mui/icons-material/Folder";
import DownloadIcon from "@mui/icons-material/Download";
import DeleteIcon from "@mui/icons-material/Delete";
import DownloadDoneIcon from "@mui/icons-material/DownloadDone";
import { PeerId, SharedFile } from "../RustCommands/ShareDirectoryContext";

import { open } from "@tauri-apps/api/dialog";
import {
  AddFiles,
  DeleteFile,
  DownloadFile,
  invokeNetworkCommand,
} from "../RustCommands/networkCommands";
import React from "react";
import { invoke } from "@tauri-apps/api";
import { ErrorContext } from "../App";

type DirectoryDetailsProps = {
  files: Map<string, SharedFile>;
  directoryName: string;
  directoryIdentifier: string;
  currentPeers: Array<PeerId>;
};

function toLargestDenominator(size: number): string {
  const sizes = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
  const k = 1024;
  const decimals = 1;

  const i = Math.floor(Math.log(size) / Math.log(k));

  return `${parseFloat((size / Math.pow(k, i)).toFixed(decimals))} ${sizes[i]}`;
}

function DirectoryDetails({
  files,
  directoryName,
  directoryIdentifier,
  currentPeers,
}: DirectoryDetailsProps) {
  const [addingFiles, setAddingFiles] = React.useState(false);
  const [fileDetails, setFileDetails] = React.useState<SharedFile | null>(null);
  const detailsOpen = Boolean(fileDetails);
  const error = React.useContext(ErrorContext);

  React.useEffect(() => {
    setAddingFiles(false);
  }, [files]);

  React.useEffect(() => {
    setAddingFiles(false);
  }, [error]);

  const handleAddFiles = async () => {
    const selected = await open({
      multiple: true,
    });

    if (selected) {
      const files: string[] = Array.isArray(selected) ? selected : [selected];

      const request: AddFiles = {
        addFiles: {
          file_paths: files,
          directory_identifier: directoryIdentifier,
        },
      };

      await invokeNetworkCommand(request);
      setAddingFiles(true);
    }
  };

  const handleOpenFileDetails = (file: SharedFile) => () => {
    setFileDetails(file);
  };

  const handleCloseFileDetails = () => {
    setFileDetails(null);
  };

  const handleDownload = (fileId: string) => async () => {
    const file = files.get(fileId);

    if (!file) return;

    const request: DownloadFile = {
      downloadFile: {
        file_identifier: fileId,
        directory_identifier: directoryIdentifier,
      },
    };

    await invokeNetworkCommand(request);
  };

  const handleDelete = (fileId: string) => () => {
    const file = files.get(fileId);

    if (!file) return;

    const request: DeleteFile = {
      deleteFile: {
        file_identifier: fileId,
        directory_identifier: directoryIdentifier,
      },
    };

    invokeNetworkCommand(request).finally(() => {
      handleCloseFileDetails();
    });
  };

  const handleOpenFile = (file: SharedFile) => async () => {
    if (file?.contentLocation?.localPath) {
      const result = await invoke("open_file", {
        message: {
          file_path: file.contentLocation.localPath,
        },
      });
    }
  };

  let rows = [];
  for (const [id, file] of files.entries()) {
    const fileIsDownloadable = file.ownedPeers.find((peer) => {
      for (const connectedPeer of currentPeers) {
        if (
          connectedPeer.hostname === peer.hostname &&
          connectedPeer.uuid === peer.uuid
        ) {
          return true;
        }
      }

      return false;
    });

    const downloadButton = fileIsDownloadable ? (
      <IconButton onClick={handleDownload(file.identifier)} color="success">
        <DownloadIcon />
      </IconButton>
    ) : (
      <IconButton color="error">
        <DownloadIcon />
      </IconButton>
    );

    const fileButton = file?.contentLocation?.localPath ? (
      <IconButton onClick={handleOpenFile(file)}>
        <DownloadDoneIcon />
      </IconButton>
    ) : (
      downloadButton
    );

    const row = (
      <TableRow key={id}>
        <TableCell variant="body">{file.name}</TableCell>
        <TableCell variant="body" align="right">
          {toLargestDenominator(file.size)}
        </TableCell>
        <TableCell variant="body" align="right">
          {new Date(file.lastModified).toLocaleString()}
        </TableCell>
        <TableCell variant="body" align="right">
          <IconButton onClick={handleOpenFileDetails(file)}>
            <InfoRoundedIcon />
          </IconButton>
        </TableCell>
        <TableCell variant="body" align="right">
          {fileButton}
        </TableCell>
      </TableRow>
    );

    rows.push(row);
  }

  if (rows.length < 1) {
    const emptyFileRow = (
      <TableRow key={1}>
        <TableCell align="center" colSpan={5}>
          No files found in this directory
        </TableCell>
      </TableRow>
    );

    rows.push(emptyFileRow);
  }

  return (
    <React.Fragment>
      <Backdrop
        sx={{ color: "#fff", zIndex: (theme: any) => theme.zIndex.drawer + 1 }}
        open={addingFiles}
      >
        <CircularProgress color="inherit" />
      </Backdrop>
      <Box sx={{ height: "100%", margin: "1em", marginRight: "2.5em" }}>
        <Box
          display={"flex"}
          justifyContent={"space-between"}
          marginTop={"1em"}
          marginBottom={"1em"}
        >
          <Breadcrumbs
            style={{
              margin: "0.25em 0.25em 1em 0.25em",
              padding: "0.5em",
              alignItems: "center",
            }}
            separator={<NavigateNextRoundedIcon fontSize="small" />}
          >
            <FolderIcon />
            <Link underline="hover" color="info">
              {directoryName}
            </Link>
          </Breadcrumbs>
          <Button variant="contained" onClick={handleAddFiles} size="small">
            Add Files
          </Button>
        </Box>
        <TableContainer component={Paper} elevation={2} variant="elevation">
          <Table
            style={{
              height: "100%",
            }}
          >
            <TableHead>
              <TableRow>
                <TableCell variant="head">File Name</TableCell>
                <TableCell variant="head" align="right">
                  Size
                </TableCell>
                <TableCell variant="head" align="right">
                  Date Shared
                </TableCell>
                <TableCell variant="head" align="right">
                  Details
                </TableCell>
                <TableCell></TableCell>
              </TableRow>
            </TableHead>
            <TableBody>{rows}</TableBody>
            <TableFooter>
              <TableRow>
                <TableCell colSpan={5}></TableCell>
              </TableRow>
            </TableFooter>
          </Table>
        </TableContainer>
      </Box>
      {fileDetails && (
        <Dialog open={detailsOpen} onClose={handleCloseFileDetails}>
          <div>
            <DialogTitle>Details for {fileDetails.name}</DialogTitle>
            <DialogContent>
              {fileDetails.contentLocation &&
                fileDetails.contentLocation.localPath && (
                  <Box marginBottom={"1em"}>
                    <Typography variant="caption" color={"GrayText"}>
                      Local path:
                    </Typography>
                    <Typography variant="body1">
                      {fileDetails.contentLocation.localPath}
                    </Typography>
                  </Box>
                )}
              {fileDetails.ownedPeers && fileDetails.ownedPeers.length > 0 && (
                <Box>
                  <Typography variant="caption" color={"GrayText"}>
                    Devices that have this file:
                  </Typography>
                  {fileDetails.ownedPeers.map((peer) => {
                    return (
                      <Typography key={peer.uuid}>{peer.hostname}</Typography>
                    );
                  })}
                </Box>
              )}
            </DialogContent>
            <DialogActions>
              <Button onClick={handleCloseFileDetails}>Close</Button>
              {fileDetails.contentLocation &&
                fileDetails.contentLocation.localPath && (
                  <Button
                    onClick={handleDelete(fileDetails.identifier)}
                    color="error"
                  >
                    <DeleteIcon />
                  </Button>
                )}
            </DialogActions>
          </div>
        </Dialog>
      )}
    </React.Fragment>
  );
}

export default DirectoryDetails;
