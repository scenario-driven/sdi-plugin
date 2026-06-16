import { clsx, type ClassValue } from 'clsx';
import { extendTailwindMerge } from 'tailwind-merge';

const TYPOGRAPHY_SIZES = [
  'display-2xl',
  'display-xl',
  'headline-lg',
  'headline-md',
  'body-lg',
  'body-base',
  'body-sm',
  'label-sm',
] as const;

const twMergeCustom = extendTailwindMerge({
  extend: {
    classGroups: {
      'font-size': [{ text: [...TYPOGRAPHY_SIZES] }],
    },
  },
});

export function cn(...inputs: ClassValue[]): string {
  return twMergeCustom(clsx(inputs));
}
