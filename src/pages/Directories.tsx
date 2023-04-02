import { appWindow } from "@tauri-apps/api/window";
import ResizableBox from "../Components/ResizableBox";

import './Directories.css';

function Directories() {

  const leftContainer = <div className="directories"></div>;
  const rightContainer = <div className="directory-details"></div>;

  return (
    <div className='directories-view'>
      <ResizableBox
        leftContainer={leftContainer}
        rightContainer={rightContainer}
        minWidth={"10vw"} 
        maxWidth={"80vw"}
      />
    </div>
  );
}

export default Directories;
