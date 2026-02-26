import { Component } from "react";

interface Props {
  elements?: unknown[];
  stylesheet?: unknown[];
  style?: React.CSSProperties;
  cy?: (cy: unknown) => void;
  [key: string]: unknown;
}

export default class CytoscapeComponent extends Component<Props> {
  render() {
    return <div data-testid="cytoscape-graph" style={this.props.style} />;
  }
}
