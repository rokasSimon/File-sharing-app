import {
  Box,
  Breadcrumbs,
  Button,
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
import { SharedFile } from "../RustCommands/ShareDirectoryContext";

import { open } from "@tauri-apps/api/dialog";
import {
  AddFiles,
  DeleteFile,
  DownloadFile,
  invokeNetworkCommand,
} from "../RustCommands/networkCommands";
import React from "react";

type DirectoryDetailsProps = {
  files: Map<string, SharedFile>;
  directoryName: string;
  directoryIdentifier: string;
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
}: DirectoryDetailsProps) {
  const [fileDetails, setFileDetails] = React.useState<SharedFile | null>(null);
  const detailsOpen = Boolean(fileDetails);

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
    }
  };

  const handleOpenFileDetails = (file: SharedFile) => () => {
    setFileDetails(file);
  };

  const handleCloseFileDetails = () => {
    setFileDetails(null);
  };

  const handleDownload = (fileId: string) => () => {
    const file = files.get(fileId);

    if (!file) return;

    const request: DownloadFile = {
      fileIdentifier: fileId
    };

    
  };

  const handleDelete = (fileId: string) => () => {
    const file = files.get(fileId);

    if (!file) return;

    const request: DeleteFile = {
      fileIdentifier: fileId
    };

    invokeNetworkCommand(request).then((value) => {
      handleCloseFileDetails();
    });
  };

  let rows = [];
  for (const [id, file] of files.entries()) {
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
          <IconButton onClick={handleDownload(file.identifier)}>
            <DownloadIcon />
          </IconButton>
        </TableCell>
      </TableRow>
    );

    rows.push(row);
  }

  if (rows.length < 1) {
    const emptyFileRow = (
      <TableRow>
        <TableCell align="center" colSpan={5}>
          No files found in this directory
        </TableCell>
      </TableRow>
    );

    rows.push(emptyFileRow);
  }

  return (
    <React.Fragment>
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
                  <Box marginBottom={'1em'}>
                    <Typography variant="caption" color={"GrayText"}>Local path:</Typography>
                    <Typography variant="body1">
                      {fileDetails.contentLocation.localPath}
                    </Typography>
                  </Box>
                )}
              {fileDetails.ownedPeers && fileDetails.ownedPeers.length > 0 && (
                <div>
                  <Typography variant="caption" color={"GrayText"}>Devices that have this file:</Typography>
                  {fileDetails.ownedPeers.map((peer) => {
                      return (
                        <Typography key={peer.uuid}>
                          {peer.hostname}
                        </Typography>
                      );
                    })}
                </div>
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
