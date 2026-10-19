/*
 * Linear-light 1/2 shrink-on-load for the luma component (issue #60).
 *
 * At scale 1/2 libjpeg runs a reduced 4x4 IDCT on luma. That is close to
 * averaging each 2x2 of the full decode, but in gamma space, and gamma
 * averaging is what the linear-light resize exists to avoid. This hook
 * keeps the full 8x8 IDCT -- the same routine a full-size decode uses --
 * and averages each 2x2 in linear light, through the sRGB transfer
 * function applied to Y'.
 *
 * Installed per decoder after jpeg_start_decompress. The state lives in
 * the decoder's own image pool and is reached through client_data, so
 * concurrent decoders share nothing.
 */

#include <stddef.h>
#include <stdint.h>
#include <stdio.h>

#include "jpeglib.h"

typedef void (*idct_method)(j_decompress_ptr, jpeg_component_info *, JCOEFPTR,
                            JSAMPARRAY, JDIMENSION);

/* The inverse-DCT module's public face, as jpegint.h defines it. */
struct jpeg_inverse_dct {
  void (*start_pass)(j_decompress_ptr cinfo);
  idct_method inverse_DCT[MAX_COMPONENTS];
};

/* What a full-size ISLOW decode uses (jddctmgr.c makes the same choice). */
extern int jsimd_can_idct_islow(void);
extern void jsimd_idct_islow(j_decompress_ptr, jpeg_component_info *,
                             JCOEFPTR, JSAMPARRAY, JDIMENSION);
extern void jpeg_idct_islow(j_decompress_ptr, jpeg_component_info *,
                            JCOEFPTR, JSAMPARRAY, JDIMENSION);

struct linear_shrink {
  idct_method full; /* the 8x8 IDCT a full-size decode would run */
  const uint16_t *pair_linear; /* 65536 entries: lin(lo byte) + lin(hi byte) */
  const uint8_t *to_srgb;      /* 16384 entries */
};

static void linear_half(j_decompress_ptr cinfo, jpeg_component_info *comp,
                        JCOEFPTR coef, JSAMPARRAY out, JDIMENSION col) {
  const struct linear_shrink *s = (const struct linear_shrink *)cinfo->client_data;
  JSAMPLE block[DCTSIZE2];
  JSAMPROW rows[DCTSIZE];
  for (int y = 0; y < DCTSIZE; y++)
    rows[y] = block + y * DCTSIZE;
  s->full(cinfo, comp, coef, rows, 0);
  for (int y = 0; y < DCTSIZE / 2; y++) {
    const JSAMPLE *r0 = block + 2 * y * DCTSIZE, *r1 = r0 + DCTSIZE;
    JSAMPLE *o = out[y] + col;
    for (int x = 0; x < DCTSIZE / 2; x++) {
      unsigned sum = s->pair_linear[r0[2 * x] | (r0[2 * x + 1] << 8)] +
                     s->pair_linear[r1[2 * x] | (r1[2 * x + 1] << 8)];
      o[x] = s->to_srgb[sum >> 2];
    }
  }
}

/*
 * Returns 1 when the hook is installed: luma decodes at 4/8 with the
 * integer IDCT. Returns 0, leaving the decoder untouched, otherwise.
 */
int oximg_linear_shrink_install(j_decompress_ptr cinfo,
                                const uint16_t *pair_linear,
                                const uint8_t *to_srgb) {
  if (cinfo->num_components < 1 || cinfo->dct_method != JDCT_ISLOW)
    return 0;
  jpeg_component_info *luma = &cinfo->comp_info[0];
#if JPEG_LIB_VERSION >= 70
  if (luma->DCT_h_scaled_size != 4 || luma->DCT_v_scaled_size != 4)
    return 0;
#else
  if (luma->DCT_scaled_size != 4)
    return 0;
#endif
  struct linear_shrink *s = (struct linear_shrink *)(*cinfo->mem->alloc_small)(
      (j_common_ptr)cinfo, JPOOL_IMAGE, sizeof(struct linear_shrink));
  s->full = jsimd_can_idct_islow() ? jsimd_idct_islow : jpeg_idct_islow;
  s->pair_linear = pair_linear;
  s->to_srgb = to_srgb;
  cinfo->client_data = s;
  cinfo->idct->inverse_DCT[0] = linear_half;
  return 1;
}
