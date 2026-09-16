#!/usr/bin/env clojure

"USAGE: ./release.clj"

;; Single source of truth for the release version. Bump this before running.
(def version "1.1.2")

(assert (re-matches #"\d+\.\d+\.\d+" version)
        "release.clj must define a semantic version")

(println "Releasing version" version)

(require '[clojure.string :as str])
(require '[clojure.java.shell :as sh])

(defn update-file [f fn]
  (print "Updating" (str f "...")) (flush)
  (spit f (fn (slurp f)))
  (println "OK"))

(defn replace-version
  "Return an update-file transform that rewrites the version matched by
  pattern. The replacement string may reference the pattern's capture groups."
  [pattern replacement]
  (fn [content] (str/replace content pattern replacement)))

(defn set-version [pattern]
  (replace-version pattern (str "$1" version)))

(def rust-crates ["dtlvnative" "dtlvnative-sys" "dtlvnative-build"])

(defn update-rust-lock []
  ;; The workspace version appears in Cargo.lock for each member crate, so
  ;; rewrite only those entries to avoid touching unrelated dependency versions.
  (update-file "src/rust/Cargo.lock"
    (fn [content]
      (reduce
        (fn [text crate]
          (str/replace text
            (re-pattern (str "(name = \"" crate "\"\\nversion = \")[^\"]+(\")"))
            (str "$1" version "$2")))
        content
        rust-crates))))

(def ^:dynamic *env* {})

(defn sh [& args]
  (apply println "Running" (if (empty? *env*) "" (str :env " " *env*)) args)
  (let [res (apply sh/sh (concat args [:env (merge (into {} (System/getenv)) *env*)]))]
    (if (== 0 (:exit res))
      (do
        (println (:out res))
        (:out res))
      (binding [*out* *err*]
        (println "Process" args "exited with code" (:exit res))
        (println (:out res))
        (println (:err res))
        (throw (ex-info (str "Process" args "exited with code" (:exit res)) res))))))

(defn update-version []
  (println "\n\n[ Updating version number ]\n")
  (update-file "CHANGELOG.md" #(str/replace % "# WIP" (str "# " version)))

  ;; JVM platform packages.
  (update-file "windows-x86_64/project.clj"
               (set-version #"(\(def version \")[0-9.]+(?=\")"))
  (doseq [file ["freebsd-x86_64/project.clj"
                "linux-arm64/project.clj"
                "linux-x86_64/project.clj"
                "macosx-arm64/project.clj"]]
    (update-file file
                 (set-version #"(\(defproject org\.clojars\.huahaiy/dtlvnative-\S+ \")[0-9.]+(?=\")")))

  ;; Rust source crates ship at the same version as the JVM packages.
  (update-file "src/rust/Cargo.toml"
               (set-version #"(?m)(^version = \")[0-9.]+(?=\")"))
  (update-file "src/rust/dtlvnative/Cargo.toml"
               (set-version #"(dtlvnative-sys = \{ path = \"\.\./dtlvnative-sys\", version = \"=)[0-9.]+(?=\")"))
  (update-file "src/rust/dtlvnative-sys/native-artifacts.json"
               (set-version #"(\"(?:version|release_tag)\": \")[0-9.]+(?=\")"))
  (update-rust-lock)
  (doseq [file ["README.md" "src/rust/README.md" "src/rust/dtlvnative/README.md"]]
    (update-file file (set-version #"(dtlvnative = \{ version = \")[0-9.]+(?=\")"))
    (update-file file (set-version #"(dtlvnative = \")[0-9.]+(?=\")"))
    (update-file file (set-version #"(releases/tag/)[0-9.]+"))))

(defn make-commit []
  (println "\n\n[ Making a commit ]\n")
  (sh "git" "add"
      "CHANGELOG.md"
      "release.clj"
      "README.md"
      "freebsd-x86_64/project.clj"
      "linux-arm64/project.clj"
      "linux-x86_64/project.clj"
      "macosx-arm64/project.clj"
      "windows-x86_64/project.clj"
      "src/rust/Cargo.toml"
      "src/rust/Cargo.lock"
      "src/rust/README.md"
      "src/rust/dtlvnative/Cargo.toml"
      "src/rust/dtlvnative/README.md"
      "src/rust/dtlvnative-sys/native-artifacts.json"
      )

  (sh "git" "commit" "-m" (str "Version " version))
  (sh "git" "tag" "--no-sign" version)
  (sh "git" "push" "origin" "master"))

(defn- str->json [s]
  (-> s
      (str/replace "\\" "\\\\")
      (str/replace "\"" "\\\"")
      (str/replace "\n" "\\n")))

(defn- map->json [m]
  (str "{ "
    (->>
      (map (fn [[k v]] (str "\"" (str->json k) "\": \"" (str->json v) "\"")) m)
      (str/join ",\n"))
    " }"))

(def GITHUB_AUTH (System/getenv "GITHUB_AUTH"))

(defn github-release []
  (let [changelog (->> (slurp "CHANGELOG.md")
                       str/split-lines
                       (drop-while #(not= (str "# " version) %))
                       next
                       (take-while #(not (re-matches #"# .+" %)))
                       (remove str/blank?)
                       (str/join "\n"))
        request   {"tag_name"         version
                   "name"             version
                   "target_commitish" "master"
                   "body"             changelog}]
    (sh "curl" "-u" GITHUB_AUTH
        "-X" "POST"
        "--data" (map->json request)
        "https://api.github.com/repos/datalevin/dtlvnative/releases")))

(defn -main []
  (update-version)
  (make-commit)
  (github-release)
  (System/exit 0))

(-main)
