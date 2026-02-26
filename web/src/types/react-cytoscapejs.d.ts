declare module "react-cytoscapejs" {
  import type { Component } from "react";
  import type cytoscape from "cytoscape";

  interface CytoscapeComponentProps {
    elements: cytoscape.ElementDefinition[];
    stylesheet?: cytoscape.StylesheetStyle[];
    style?: React.CSSProperties;
    cy?: (cy: cytoscape.Core) => void;
    layout?: cytoscape.LayoutOptions;
    minZoom?: number;
    maxZoom?: number;
    boxSelectionEnabled?: boolean;
    userPanningEnabled?: boolean;
    userZoomingEnabled?: boolean;
    pan?: cytoscape.Position;
    zoom?: number;
    autoungrabify?: boolean;
    autounselectify?: boolean;
  }

  export default class CytoscapeComponent extends Component<CytoscapeComponentProps> {}
}
