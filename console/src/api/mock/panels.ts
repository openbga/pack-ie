import { formatScaled } from "./util";

export type PanelKey =
  | "chem"
  | "hema"
  | "coag"
  | "urine"
  | "immuno"
  | "gases"
  | "hba1c"
  | "poc"
  | "order";

/** One analyte. Reference limits are integers scaled by `10^decimals`: exact, never floats. */
interface TestDef {
  code: string;
  name: string;
  unit: string;
  decimals: number;
  low: number;
  high: number;
}

export interface Panel {
  /** Sending application name used in HL7 MSH-3 and ASTM H records. */
  analyzer: string;
  specimen: string;
  tests: TestDef[];
}

function test(
  code: string,
  name: string,
  unit: string,
  decimals: number,
  low: number,
  high: number,
): TestDef {
  return { code, name, unit, decimals, low, high };
}

const CHEM: TestDef[] = [
  test("GLU", "Glucose", "mmol/L", 1, 39, 61),
  test("BUN", "Urea nitrogen", "mmol/L", 1, 28, 72),
  test("CREA", "Creatinine", "umol/L", 0, 60, 110),
  test("NA", "Sodium", "mmol/L", 0, 136, 145),
  test("K", "Potassium", "mmol/L", 1, 35, 51),
  test("CL", "Chloride", "mmol/L", 0, 98, 107),
  test("ALT", "Alanine aminotransferase", "U/L", 0, 7, 40),
  test("AST", "Aspartate aminotransferase", "U/L", 0, 10, 40),
  test("TBIL", "Total bilirubin", "umol/L", 0, 3, 21),
  test("ALB", "Albumin", "g/L", 0, 35, 52),
  test("CA", "Calcium", "mmol/L", 2, 215, 255),
  test("CRP", "C-reactive protein", "mg/L", 1, 0, 50),
];

export const PANELS: Record<PanelKey, Panel> = {
  chem: { analyzer: "CHEM", specimen: "SER", tests: CHEM },
  hema: {
    analyzer: "HEMA",
    specimen: "BLD",
    tests: [
      test("WBC", "White blood cells", "10*9/L", 1, 40, 110),
      test("RBC", "Red blood cells", "10*12/L", 2, 400, 580),
      test("HGB", "Hemoglobin", "g/L", 0, 120, 170),
      test("HCT", "Hematocrit", "L/L", 2, 36, 50),
      test("MCV", "Mean cell volume", "fL", 1, 800, 980),
      test("PLT", "Platelets", "10*9/L", 0, 150, 400),
    ],
  },
  coag: {
    analyzer: "COAG",
    specimen: "PLAS",
    tests: [
      test("PT", "Prothrombin time", "s", 1, 110, 145),
      test("INR", "INR", "", 2, 90, 120),
      test("APTT", "Activated PTT", "s", 1, 250, 370),
      test("FIB", "Fibrinogen", "g/L", 1, 20, 40),
    ],
  },
  urine: {
    analyzer: "URINE",
    specimen: "UR",
    tests: [
      test("SG", "Specific gravity", "", 3, 1005, 1030),
      test("PH", "Urine pH", "", 1, 50, 80),
      test("LEU", "Leukocytes", "cells/uL", 0, 0, 25),
      test("PRO", "Protein", "mg/dL", 0, 0, 15),
    ],
  },
  immuno: {
    analyzer: "IMMUNO",
    specimen: "SER",
    tests: [
      test("TSH", "Thyrotropin", "mIU/L", 2, 40, 400),
      test("FT4", "Free thyroxine", "pmol/L", 1, 120, 220),
      test("TNI", "Troponin I", "ng/L", 0, 0, 14),
      test("FERR", "Ferritin", "ug/L", 0, 30, 300),
    ],
  },
  gases: {
    analyzer: "GASES",
    specimen: "BLDA",
    tests: [
      test("PH", "Blood pH", "", 2, 735, 745),
      test("PCO2", "pCO2", "mmHg", 0, 35, 45),
      test("PO2", "pO2", "mmHg", 0, 80, 100),
      test("LAC", "Lactate", "mmol/L", 1, 5, 20),
    ],
  },
  hba1c: {
    analyzer: "HBA1C",
    specimen: "BLD",
    tests: [test("HBA1C", "Hemoglobin A1c", "%", 1, 40, 60)],
  },
  poc: {
    analyzer: "POC",
    specimen: "BLDC",
    tests: [test("GLU", "Glucose", "mmol/L", 1, 39, 61)],
  },
  order: { analyzer: "ORDERS", specimen: "SER", tests: CHEM },
};

export interface ResultValue {
  test: TestDef;
  /** Integer scaled by `10^decimals`. */
  scaled: number;
  text: string;
  /** Reference range text, such as `3.9-6.1`. */
  range: string;
  /** HL7 abnormal flag: `N`, `L` or `H`. */
  flag: "N" | "L" | "H";
}

/** The `index`-th result of a panel for a hash value: about one in five is out of range. */
export function resultFor(panel: PanelKey, index: number, hash: number): ResultValue {
  const tests = PANELS[panel].tests;
  const def = tests[index % tests.length] as TestDef;
  const span = Math.max(def.high - def.low, 2);
  const lowest = def.low - Math.floor(span / 4);
  const scaled = Math.max(0, lowest + (hash % (span + Math.floor(span / 2) + 1)));
  const flag = scaled < def.low ? "L" : scaled > def.high ? "H" : "N";
  return {
    test: def,
    scaled,
    text: formatScaled(scaled, def.decimals),
    range: `${formatScaled(def.low, def.decimals)}-${formatScaled(def.high, def.decimals)}`,
    flag,
  };
}
