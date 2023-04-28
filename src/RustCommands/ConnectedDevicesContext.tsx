import React from "react";
import { PeerId } from "./ShareDirectoryContext";
import { listen } from "@tauri-apps/api/event";
import { invokeNetworkCommand } from "./networkCommands";

type GetPeers = {
  getPeers: boolean;
};

const initialState: Array<PeerId> = [];
const ConnectedDevicesContext =
  React.createContext<Array<PeerId>>(initialState);

function ConnectedDevicesProvider({ children }: any) {
  const [peers, setPeers] = React.useState(initialState);
  const peersRef = React.useRef(peers);
  const loaded = React.useRef(false);

  React.useEffect(() => {
    peersRef.current = peers;
  }, [peers]);

  React.useEffect(() => {
    if (loaded.current) return;

    const startListenPeers = async () => {
      const _ = await listen<Array<PeerId>>("GetPeers", (event) => {
        const input = event.payload;

        peersRef.current = [...input];
        console.log("Setting peers " + JSON.stringify(peersRef.current));

        setPeers(peersRef.current);
      });
    };

    const loadPeers = async () => {
      const request: GetPeers = {
        getPeers: true,
      };

      await invokeNetworkCommand(request);
    };

    startListenPeers();
    loadPeers();

    loaded.current = true;
  }, []);

  return (
    <ConnectedDevicesContext.Provider value={peers}>
      {children}
    </ConnectedDevicesContext.Provider>
  );
}

export type {GetPeers};
export { ConnectedDevicesProvider, ConnectedDevicesContext };