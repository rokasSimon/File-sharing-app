import React from "react";

import "./ResizableBox.css";

function ResizableBox(props: {
  leftContainer: JSX.Element;
  rightContainer: JSX.Element | null;
  minWidth: number | string;
  maxWidth: number | string;
}) {
  const sidebarRef = React.useRef<HTMLDivElement>(null);
  const [isResizing, setIsResizing] = React.useState(false);
  const [sidebarWidth, setSidebarWidth] = React.useState(250);

  const startResizing = React.useCallback(() => {
    setIsResizing(true);
  }, []);

  const stopResizing = React.useCallback(() => {
    setIsResizing(false);
  }, []);

  const resize = React.useCallback(
    (mouseMoveEvent: MouseEvent) => {
      if (isResizing) {
        setSidebarWidth(
          mouseMoveEvent.clientX -
            sidebarRef.current!.getBoundingClientRect().left
        );
      }
    },
    [isResizing]
  );

  React.useEffect(() => {
    window.addEventListener("mousemove", resize);
    window.addEventListener("mouseup", stopResizing);
    return () => {
      window.removeEventListener("mousemove", resize);
      window.removeEventListener("mouseup", stopResizing);
    };
  }, [resize, stopResizing]);

  return (
    <div className="app-container">
      <div
        ref={sidebarRef}
        className="app-sidebar"
        style={{
          width: sidebarWidth,
          minWidth: props.minWidth,
          maxWidth: props.maxWidth,
        }}
        onMouseDown={(e) => e.preventDefault()}
      >
        <div className="app-sidebar-content">{props.leftContainer}</div>
        <div className="app-sidebar-resizer" onMouseDown={startResizing} />
      </div>
      <div className="app-frame">{props.rightContainer}</div>
    </div>
  );
}

export default ResizableBox;
