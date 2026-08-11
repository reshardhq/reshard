import {
  forwardRef,
  type ForwardRefExoticComponent,
  type RefAttributes,
  type SVGProps,
} from "react";
import { HugeiconsIcon } from "@hugeicons/react";
import type { IconSvgElement } from "@hugeicons/react";
import {
  Add01Icon,
  ArrowDown01Icon,
  BotIcon,
  BubbleChatIcon,
  Cancel01Icon,
  CheckIcon,
  ChevronDownIcon,
  CircleIcon,
  ComputerArrowDownIcon,
  CopyIcon,
  DicesIcon,
  Download01Icon,
  GitBranchIcon,
  InformationCircleIcon,
  MoreHorizontalIcon,
  Plug02Icon,
  RadioIcon,
  SentIcon,
  ServerStack01Icon,
} from "@hugeicons/core-free-icons";

export type IconProps = SVGProps<SVGSVGElement> & {
  size?: string | number;
  strokeWidth?: number;
  absoluteStrokeWidth?: boolean;
};

const IconRenderer = HugeiconsIcon as unknown as ForwardRefExoticComponent<
  IconProps & { icon: IconSvgElement } & RefAttributes<SVGSVGElement>
>;

function createIcon(icon: IconSvgElement, name: string) {
  const Icon = forwardRef<SVGSVGElement, IconProps>((props, ref) => (
    <IconRenderer ref={ref} icon={icon} {...props} />
  ));
  Icon.displayName = name;
  return Icon;
}

export const ArrowDown = createIcon(ArrowDown01Icon, "ArrowDown");
export const Bot = createIcon(BotIcon, "Bot");
export const Check = createIcon(CheckIcon, "Check");
export const ChevronDown = createIcon(ChevronDownIcon, "ChevronDown");
export const Circle = createIcon(CircleIcon, "Circle");
export const Copy = createIcon(CopyIcon, "Copy");
export const Dices = createIcon(DicesIcon, "Dices");
export const Download = createIcon(Download01Icon, "Download");
export const GitBranch = createIcon(GitBranchIcon, "GitBranch");
export const Info = createIcon(InformationCircleIcon, "Info");
export const MessageSquareText = createIcon(BubbleChatIcon, "MessageSquareText");
export const MonitorDown = createIcon(ComputerArrowDownIcon, "MonitorDown");
export const MoreHorizontal = createIcon(MoreHorizontalIcon, "MoreHorizontal");
export const Plus = createIcon(Add01Icon, "Plus");
export const Radio = createIcon(RadioIcon, "Radio");
export const Send = createIcon(SentIcon, "Send");
export const Server = createIcon(ServerStack01Icon, "Server");
export const Unplug = createIcon(Plug02Icon, "Unplug");
export const X = createIcon(Cancel01Icon, "X");
