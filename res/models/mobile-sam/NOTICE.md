# Bundled Smart Cutout model

MobileSAM image encoder and SAM mask decoder, converted to ONNX by Acly.
The files are unmodified. RustyPaint runs them locally with ONNX Runtime (MIT), using
ort (MIT OR Apache-2.0), or the portable tract backend (MIT OR Apache-2.0).

Source: https://huggingface.co/Acly/MobileSAM/tree/0d3b403339b4674a82493d5e97964dd78089ddc8
Original weights: https://huggingface.co/dhkim2810/MobileSAM
Architecture and export code: https://github.com/ChaoningZhang/MobileSAM
Original SAM: https://github.com/facebookresearch/segment-anything
TinyViT: https://github.com/microsoft/Cream/tree/main/TinyViT

The original weights and conversion model cards specify MIT. The upstream MobileSAM and
SAM repositories carry Apache-2.0. Both licence texts are included below in the application.
TinyViT is Copyright (c) 2022 Microsoft. SAM is Copyright (c) Meta Platforms, Inc. and affiliates.
MobileSAM authors: Chaoning Zhang, Dongshen Han, Yu Qiao, Jung Uk Kim, Sung Ho Bae,
Seungkyu Lee and Choong Seon Hong. ONNX conversion: Acly.

encoder.onnx: mobile_sam_image_encoder.onnx, 28157093 bytes
SHA-256: 580f5fb648ea1062c0aabc26217aed56921985f03f0cbbd852bba81d760cc749
decoder.onnx: sam_mask_decoder_multi.onnx, 16496559 bytes
SHA-256: 8976b90a87ba50a6a72217a5ff994f7d25ce16f2229fcc1ed259e1294c622ffe

Replacement models must provide encoder.onnx and decoder.onnx in a local folder.
The encoder accepts float32 RGB [1024,1024,3] in 0..255, includes SAM normalization,
and returns [1,256,64,64]. The decoder uses SAM's six named inputs and exposes
iou_predictions and low_res_masks [1,N,256,256], with dynamic point counts.
Use the same embedding and coordinate conventions. Arbitrary ONNX models are not compatible.

No model or image is downloaded or uploaded by Smart Cutout. The model is embedded in each build.
