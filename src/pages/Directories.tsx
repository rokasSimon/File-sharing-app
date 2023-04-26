import "./Directories.css";

import ResizableBox from "../Components/ResizableBox";
import React from "react";
import Dialog from "@mui/material/Dialog";
import {
  Box,
  Button,
  DialogActions,
  DialogContent,
  DialogTitle,
  List,
  ListItemButton,
  ListItemText,
  ListSubheader,
  Paper,
  TextField,
  Typography,
} from "@mui/material";
import {
  CreateShareDirectory,
  invokeNetworkCommand,
} from "../RustCommands/networkCommands";
import {
  ShareDirectory,
  ShareDirectoryContext,
  SharedFile,
} from "../RustCommands/ShareDirectoryContext";
import DirectoryDetails from "../Components/DirectoryDetails";

function Directories() {
  const shareDirectories = React.useContext(ShareDirectoryContext);
  const [selectedDirectory, setSelectedDirectory] = React.useState<
    ShareDirectory | undefined
  >(undefined);
  const [shareCreationName, setShareCreationName] = React.useState("");
  const [shareCreationOpen, setShareCreationOpen] = React.useState(false);

  const handleOpen = () => {
    setShareCreationName("");
    setShareCreationOpen(true);
  };
  const handleClose = () => {
    setShareCreationName("");
    setShareCreationOpen(false);
  };

  const handleNameChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setShareCreationName(event.currentTarget.value);
  };

  const handleCreate = async () => {
    let request: CreateShareDirectory = {
      createShareDirectory: shareCreationName,
    };

    try {
      await invokeNetworkCommand(request);
    } catch (e) {
      console.log(e);
    }

    handleClose();
  };

  const handleListClick = async (identifier: string) => {
    const directory = shareDirectories.find((dir) => {
      return dir.signature.identifier === identifier;
    });

    setSelectedDirectory(directory);
  };

  const duplicatedNames = new Map<string, number>();
  let directories = null;
  if (shareDirectories) {
    directories = shareDirectories.map((val, i) => {
      const usedCount = duplicatedNames.get(val.signature.name);

      if (usedCount === undefined) {
        duplicatedNames.set(val.signature.name, 1);
      } else {
        duplicatedNames.set(val.signature.name, usedCount + 1);
      }

      return (
        <ListItemButton
          style={{ maxHeight: "3em" }}
          selected={
            selectedDirectory?.signature?.identifier ===
            val.signature.identifier
          }
          divider={true}
          key={val.signature.identifier}
          onClick={() => handleListClick(val.signature.identifier)}
        >
          <ListItemText>
            {val.signature.name}
            {usedCount && (
              <Typography variant="caption" color="GrayText">{` (${usedCount})`}</Typography>
            )}
          </ListItemText>
        </ListItemButton>
      );
    });
  }

  const leftContainer = (
    <div id="directory-column">
      <div id="start-button-box">
        <Button
          onClick={handleOpen}
          variant="contained"
          id="start-share-directory-btn"
        >
          Start Share Directory
        </Button>
      </div>
      <List id="directories">{directories}</List>
    </div>
  );

  const rightContainer = selectedDirectory ? (
    <DirectoryDetails
      files={selectedDirectory.shared_files}
      directoryName={selectedDirectory.signature.name}
      directoryIdentifier={selectedDirectory.signature.identifier}
    />
  ) : null;

  return (
    <div id="directories-page">
      <div id="directories-view">
        <ResizableBox
          leftContainer={leftContainer}
          rightContainer={rightContainer}
          minWidth={"10vw"}
          maxWidth={"80vw"}
        />
      </div>
      <Dialog open={shareCreationOpen} onClose={handleClose}>
        <div>
          <DialogTitle>Creating New Share Directory</DialogTitle>
          <DialogContent>
            <TextField
              id="share-directory-name"
              label="Directory Name"
              variant="standard"
              value={shareCreationName}
              onChange={handleNameChange}
            />
          </DialogContent>
          <DialogActions>
            <Button onClick={handleClose}>Cancel</Button>
            <Button onClick={handleCreate}>Create</Button>
          </DialogActions>
        </div>
      </Dialog>
    </div>
  );
}

export default Directories;
