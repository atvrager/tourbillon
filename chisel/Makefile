TBN := cargo run --manifest-path ../Cargo.toml --
SRC := src/main/scala/tbn
EX  := ../examples

MODULES := $(SRC)/Counter.scala $(SRC)/ProducerConsumer.scala \
           $(SRC)/Priority.scala $(SRC)/BranchRouter.scala \
           $(SRC)/PeekTest.scala $(SRC)/AsyncCDC.scala \
           $(SRC)/Spi2TLULV2.scala $(SRC)/Marie.scala

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

$(SRC)/PeekTest.scala: tbn/peek.tbn
	$(TBN) build $< --target chisel -o $(SRC)

$(SRC)/AsyncCDC.scala: tbn/async_example.tbn
	$(TBN) build $< --target chisel -o $(SRC)

$(SRC)/Spi2TLULV2.scala: $(EX)/spi2tlul_v2.tbn
	$(TBN) build $< --target chisel -o $(SRC)

$(SRC)/Marie.scala: $(EX)/cpu_core.tbn $(EX)/marie.tbn
	$(TBN) build $(EX)/cpu_core.tbn $(EX)/marie.tbn --target chisel -o $(SRC)

test: generate
	sbt test

clean:
	rm -f $(MODULES)
	rm -rf target project/target
