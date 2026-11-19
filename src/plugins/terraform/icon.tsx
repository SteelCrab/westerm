/*
 * Logo path from Simple Icons (CC0). Terraform is a trademark of HashiCorp.
 * The logo is always the brand color, also on a button that is not pressed.
 */

export const TERRAFORM_PATH =
  "M1.44 0v7.575l6.561 3.79V3.787zm21.12 4.227l-6.561 3.791v7.574l6.56-3.787zM8.72 4.23v7.575l6.561 3.787V8.018zm0 8.405v7.575L15.28 24v-7.578z";

export const TERRAFORM_COLOR = "#844FBA";

/** The logo as an image address, for the plugin bar and the plugin manager (`WestermPlugin.iconUrl`). */
export const TERRAFORM_ICON_URL = `data:image/svg+xml,${encodeURIComponent(
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="${TERRAFORM_COLOR}" d="${TERRAFORM_PATH}"/></svg>`,
)}`;

export function TerraformLogo({ size }: { size: number }) {
  return (
    <svg className="tf-logo" aria-hidden="true" viewBox="0 0 24 24" width={size} height={size}>
      <path d={TERRAFORM_PATH} fill={TERRAFORM_COLOR} />
    </svg>
  );
}

/** The mark of an OpenTofu project: the letters, not OpenTofu's logo. */
export function TofuMark({ size }: { size: number }) {
  return (
    <span className="tf-tofu" aria-hidden="true" style={{ width: size, height: size, fontSize: Math.round(size * 0.55) }}>
      OT
    </span>
  );
}
