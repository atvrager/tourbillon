TBN := cargo run --manifest-path ../Cargo.toml --
SRC := src/main/scala/tbn
MODULES := $(SRC)/Counter.scala $(SRC)/ProducerConsumer.scala \
           $(SRC)/Priority.scala $(SRC)/BranchRouter.scala

.PHONY: generate test clean

generate: $(MODULES)

$(SRC)/Counter.scala: tbn/counter.tbn
	$(TBN) build $< --target chisel -o $(SRC)

$(SRC)/ProducerConsumer.scala: tbn/producer_consumer.tbn
	$(TBN) build $< --target chisel -o $(SRC)

$(SRC)/Priority.scala: tbn/priority.tbn
	$(TBN) build $< --target chisel -o $(SRC)

$(SRC)/BranchRouter.scala: tbn/branch.tbn
	$(TBN) build $< --target chisel -o $(SRC)

test: generate
	sbt test

clean:
	rm -f $(MODULES)
	rm -rf target project/target
