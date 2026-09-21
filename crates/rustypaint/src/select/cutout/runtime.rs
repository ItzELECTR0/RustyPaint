use anyhow::{Result, ensure};

#[cfg(all(
    feature = "native-cutout",
    not(any(target_env = "musl", all(target_os = "macos", target_arch = "x86_64")))
))]
mod backend {
    use super::*;
    use ort::{
        session::{OutputSelector, RunOptions, Session},
        value::TensorRef,
    };
    use std::sync::{Mutex, Once};

    pub struct Model {
        encoder: Mutex<Session>,
        decoder: Mutex<Session>,
    }

    impl Model {
        pub fn load(encoder: &[u8], decoder: &[u8]) -> Result<Self> {
            static INIT: Once = Once::new();
            INIT.call_once(|| {
                ort::init().with_telemetry(false).commit();
            });
            let session = |bytes: &[u8]| -> Result<Session> {
                Ok(Session::builder()?
                    .with_intra_threads(
                        std::thread::available_parallelism()
                            .map_or(1, usize::from)
                            .clamp(1, 4),
                    )
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?
                    .with_inter_threads(1)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?
                    .with_config_entry("session.intra_op.allow_spinning", "0")
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?
                    .commit_from_memory(bytes)?)
            };
            let (encoder, decoder) = (session(encoder)?, session(decoder)?);
            ensure!(
                encoder.inputs().len() == 1,
                "Expected a MobileSAM image encoder"
            );
            ensure!(
                decoder.inputs().len() == 6,
                "Expected a SAM ONNX mask decoder"
            );
            Ok(Self {
                encoder: Mutex::new(encoder),
                decoder: Mutex::new(decoder),
            })
        }

        pub fn encode(&self, image: &[f32]) -> Result<Vec<f32>> {
            let mut encoder = self
                .encoder
                .lock()
                .map_err(|_| anyhow::anyhow!("Encoder lock failed"))?;
            let output = encoder.run(ort::inputs![TensorRef::from_array_view((
                [1024usize, 1024, 3],
                image
            ))?])?;
            let (shape, data) = output[0].try_extract_tensor::<f32>()?;
            ensure!(
                shape.as_ref() == [1, 256, 64, 64],
                "Unsupported encoder embedding shape"
            );
            Ok(data.to_vec())
        }

        pub fn decode(
            &self,
            embedding: &[f32],
            coords: &[f32],
            labels: &[f32],
        ) -> Result<(Vec<f32>, Vec<f32>)> {
            let mut decoder = self
                .decoder
                .lock()
                .map_err(|_| anyhow::anyhow!("Decoder lock failed"))?;
            let options = RunOptions::new()?.with_outputs(
                OutputSelector::no_default()
                    .with("iou_predictions")
                    .with("low_res_masks"),
            );
            let zeros = vec![0.0f32; 256 * 256];
            let output = decoder.run_with_options(ort::inputs! {
                "image_embeddings" => TensorRef::from_array_view(([1usize, 256, 64, 64], embedding))?,
                "point_coords" => TensorRef::from_array_view(([1usize, labels.len(), 2], coords))?,
                "point_labels" => TensorRef::from_array_view(([1usize, labels.len()], labels))?,
                "mask_input" => TensorRef::from_array_view(([1usize, 1, 256, 256], zeros.as_slice()))?,
                "has_mask_input" => TensorRef::from_array_view(([1usize], &[0.0f32][..]))?,
                "orig_im_size" => TensorRef::from_array_view(([2usize], &[1024.0f32, 1024.0][..]))?,
            }, &options)?;
            let (_, scores) = output["iou_predictions"].try_extract_tensor::<f32>()?;
            let (shape, masks) = output["low_res_masks"].try_extract_tensor::<f32>()?;
            ensure!(
                shape.as_ref() == [1, scores.len() as i64, 256, 256],
                "Unsupported decoder mask shape"
            );
            Ok((scores.to_vec(), masks.to_vec()))
        }
    }
}

#[cfg(not(all(
    feature = "native-cutout",
    not(any(target_env = "musl", all(target_os = "macos", target_arch = "x86_64")))
)))]
mod backend {
    use super::*;
    use std::{io::Cursor, sync::Arc};
    use tract_linalg::multithread::{Executor, multithread_tract_scope};
    use tract_onnx::prelude::*;

    pub struct Model {
        encoder: Arc<TypedRunnableModel>,
        decoder: Arc<TypedRunnableModel>,
        executor: Executor,
    }

    impl Model {
        pub fn load(encoder: &[u8], decoder: &[u8]) -> Result<Self> {
            let mut encoder = tract_onnx::onnx().model_for_read(&mut Cursor::new(encoder))?;
            ensure!(
                encoder.input_outlets()?.len() == 1,
                "Expected a MobileSAM image encoder"
            );
            encoder.set_input_fact(0, f32::fact([1024, 1024, 3]).into())?;
            let encoder = encoder.into_optimized()?.into_runnable()?;
            let mut decoder = tract_onnx::onnx().model_for_read(&mut Cursor::new(decoder))?;
            ensure!(
                decoder.input_outlets()?.len() == 6,
                "Expected a SAM ONNX mask decoder"
            );
            decoder.select_outputs_by_name(["iou_predictions", "low_res_masks"])?;
            let decoder = decoder.into_optimized()?.into_runnable()?;
            let threads = std::thread::available_parallelism()
                .map_or(1, usize::from)
                .clamp(1, 4);
            Ok(Self {
                encoder,
                decoder,
                executor: Executor::multithread_with_name(threads, "cutout"),
            })
        }

        pub fn encode(&self, image: &[f32]) -> Result<Vec<f32>> {
            let input = Tensor::from_shape(&[1024, 1024, 3], image)?;
            let output = multithread_tract_scope(self.executor.clone(), || {
                self.encoder.run(tvec!(input.into()))
            })?;
            ensure!(
                output[0].shape() == [1, 256, 64, 64],
                "Unsupported encoder embedding shape"
            );
            Ok(output[0]
                .to_plain_array_view::<f32>()?
                .iter()
                .copied()
                .collect())
        }

        pub fn decode(
            &self,
            embedding: &[f32],
            coords: &[f32],
            labels: &[f32],
        ) -> Result<(Vec<f32>, Vec<f32>)> {
            let inputs = tvec!(
                Tensor::from_shape(&[1, 256, 64, 64], embedding)?.into(),
                Tensor::from_shape(&[1, labels.len(), 2], coords)?.into(),
                Tensor::from_shape(&[1, labels.len()], labels)?.into(),
                Tensor::zero::<f32>(&[1, 1, 256, 256])?.into(),
                tensor1(&[0.0f32]).into(),
                tensor1(&[1024.0f32, 1024.0]).into(),
            );
            let output =
                multithread_tract_scope(self.executor.clone(), || self.decoder.run(inputs))?;
            let scores: Vec<f32> = output[0]
                .to_plain_array_view::<f32>()?
                .iter()
                .copied()
                .collect();
            ensure!(
                output[1].shape() == [1, scores.len(), 256, 256],
                "Unsupported decoder mask shape"
            );
            Ok((
                scores,
                output[1]
                    .to_plain_array_view::<f32>()?
                    .iter()
                    .copied()
                    .collect(),
            ))
        }
    }
}

pub use backend::Model;
