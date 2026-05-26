export interface BrandMarkProps {
  size?: number;
  className?: string;
  'aria-hidden'?: boolean;
}

// SDI — Scenario-Driven Implementation. The mark is a stylised "S/D" formed by
// a rounded square ("scenario card") with a forward arrow stroke ("driven").
export function BrandMark({
  size = 24,
  className,
  'aria-hidden': ariaHidden = true,
}: BrandMarkProps) {
  return (
    <svg
      data-testid="brand-mark"
      width={size}
      height={size}
      viewBox="0 0 32 32"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden={ariaHidden}
      className={className}
    >
      <rect x="4" y="6" width="24" height="20" rx="4" fill="#0EA5E9" />
      <path
        d="M9 12h14M9 16h10M9 20h7"
        stroke="white"
        strokeWidth="2"
        strokeLinecap="round"
      />
      <path
        d="M22 18l4 4-4 4"
        stroke="#FACC15"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
