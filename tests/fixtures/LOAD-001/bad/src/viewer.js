import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { DRACOLoader } from 'three/addons/loaders/DRACOLoader.js';

const scene = new THREE.Scene();
const draco = new DRACOLoader();
draco.setDecoderPath('/draco/');

const loader = new GLTFLoader();
loader.setDRACOLoader(draco);
loader.load('/models/chair.glb', (gltf) => scene.add(gltf.scene));
