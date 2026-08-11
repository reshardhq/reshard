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
  AlertCircleIcon,
  ArrowDown01Icon,
  ArrowUpDownIcon,
  AtSignIcon as AtSignGlyph,
  AudioLinesIcon as AudioLinesGlyph,
  BadgeCheckIcon as BadgeCheckGlyph,
  BellIcon as BellGlyph,
  BellOffIcon as BellOffGlyph,
  BlocksIcon as BlocksGlyph,
  BotIcon as BotGlyph,
  BoxIcon as BoxGlyph,
  BubbleChatIcon,
  Calendar01Icon,
  Cancel01Icon,
  CheckIcon as CheckGlyph,
  ChevronDownIcon as ChevronDownGlyph,
  ChevronLeftIcon as ChevronLeftGlyph,
  ChevronRightIcon as ChevronRightGlyph,
  CopyIcon as CopyGlyph,
  CornerUpLeftIcon as CornerUpLeftGlyph,
  CreditCardIcon as CreditCardGlyph,
  Delete02Icon,
  DicesIcon as DicesGlyph,
  Home01Icon,
  HashIcon as HashGlyph,
  InboxIcon as InboxGlyph,
  InformationCircleIcon,
  Link01Icon,
  Loading03Icon,
  Logout03Icon,
  MessageQuestionIcon,
  MoreHorizontalIcon as MoreHorizontalGlyph,
  Notification01Icon,
  NotificationOff01Icon,
  PanelLeftIcon as PanelLeftGlyph,
  Plug02Icon,
  RotateLeft01Icon,
  Search01Icon,
  SentIcon,
  Settings02Icon,
  SparklesIcon as SparklesGlyph,
  TerminalIcon as TerminalGlyph,
  ZapIcon as ZapGlyph,
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

export const ArrowDownIcon = createIcon(ArrowDown01Icon, "ArrowDownIcon");
export const AtSignIcon = createIcon(AtSignGlyph, "AtSignIcon");
export const AudioLinesIcon = createIcon(AudioLinesGlyph, "AudioLinesIcon");
export const BadgeCheckIcon = createIcon(BadgeCheckGlyph, "BadgeCheckIcon");
export const BellIcon = createIcon(BellGlyph, "BellIcon");
export const BellOffIcon = createIcon(BellOffGlyph, "BellOffIcon");
export const BlocksIcon = createIcon(BlocksGlyph, "BlocksIcon");
export const BotIcon = createIcon(BotGlyph, "BotIcon");
export const BoxIcon = createIcon(BoxGlyph, "BoxIcon");
export const CalendarIcon = createIcon(Calendar01Icon, "CalendarIcon");
export const CheckIcon = createIcon(CheckGlyph, "CheckIcon");
export const ChevronDownIcon = createIcon(ChevronDownGlyph, "ChevronDownIcon");
export const ChevronLeftIcon = createIcon(ChevronLeftGlyph, "ChevronLeftIcon");
export const ChevronRightIcon = createIcon(ChevronRightGlyph, "ChevronRightIcon");
export const ChevronsUpDownIcon = createIcon(ArrowUpDownIcon, "ChevronsUpDownIcon");
export const CircleAlertIcon = createIcon(AlertCircleIcon, "CircleAlertIcon");
export const CopyIcon = createIcon(CopyGlyph, "CopyIcon");
export const CornerUpLeftIcon = createIcon(CornerUpLeftGlyph, "CornerUpLeftIcon");
export const CreditCardIcon = createIcon(CreditCardGlyph, "CreditCardIcon");
export const DicesIcon = createIcon(DicesGlyph, "DicesIcon");
export const HomeIcon = createIcon(Home01Icon, "HomeIcon");
export const HashIcon = createIcon(HashGlyph, "HashIcon");
export const InboxIcon = createIcon(InboxGlyph, "InboxIcon");
export const InfoIcon = createIcon(InformationCircleIcon, "InfoIcon");
export const LinkIcon = createIcon(Link01Icon, "LinkIcon");
export const LoaderCircleIcon = createIcon(Loading03Icon, "LoaderCircleIcon");
export const LogOutIcon = createIcon(Logout03Icon, "LogOutIcon");
export const MessageCircleQuestionIcon = createIcon(MessageQuestionIcon, "MessageCircleQuestionIcon");
export const MessageSquareIcon = createIcon(BubbleChatIcon, "MessageSquareIcon");
export const MoreHorizontalIcon = createIcon(MoreHorizontalGlyph, "MoreHorizontalIcon");
export const BellNotificationIcon = createIcon(Notification01Icon, "BellNotificationIcon");
export const NotificationOffIcon = createIcon(NotificationOff01Icon, "NotificationOffIcon");
export const PanelLeftIcon = createIcon(PanelLeftGlyph, "PanelLeftIcon");
export const PlusIcon = createIcon(Add01Icon, "PlusIcon");
export const RotateCcwIcon = createIcon(RotateLeft01Icon, "RotateCcwIcon");
export const SearchIcon = createIcon(Search01Icon, "SearchIcon");
export const SendIcon = createIcon(SentIcon, "SendIcon");
export const Settings2Icon = createIcon(Settings02Icon, "Settings2Icon");
export const SparklesIcon = createIcon(SparklesGlyph, "SparklesIcon");
export const TerminalIcon = createIcon(TerminalGlyph, "TerminalIcon");
export const Trash2Icon = createIcon(Delete02Icon, "Trash2Icon");
export const UnplugIcon = createIcon(Plug02Icon, "UnplugIcon");
export const XIcon = createIcon(Cancel01Icon, "XIcon");
export const ZapIcon = createIcon(ZapGlyph, "ZapIcon");
