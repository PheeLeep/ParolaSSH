import { useEffect, useState } from "react";
import { hostNavStyle } from "../../lib/appWindow";
import {
  readNavLayout,
  subscribeNavLayout,
  type NavLayout,
  type NavStyle,
} from "./preferences";

export function resolveNavStyle(layout: NavLayout): NavStyle {
  return layout === "auto" ? hostNavStyle : layout;
}

/** The titlebar convention the navbar should draw right now. */
export function useNavStyle(): NavStyle {
  const [layout, setLayout] = useState<NavLayout>(readNavLayout);
  useEffect(() => subscribeNavLayout(setLayout), []);
  return resolveNavStyle(layout);
}
